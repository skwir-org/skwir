//! rclone (ADR-077): the remotes of its configuration (`rclone://gdrive/`,
//! seventy kinds of storage), and SFTP, SMB, WebDAV and FTP addresses
//! (SFTP through the user's own ssh). Mounted with the cache the user
//! chose for the server, else Automatic's (ADR-102): `full`, which keeps
//! what is read and written on this disk an hour and sends a file a few
//! seconds after it is closed, but `minimal` on the local network and for
//! a remote that encrypts or wraps others (`options::automatic`). A copy is
//! done when the server has it whatever the cache: a mount that sends
//! later is asked to send what a plan wrote, and waited for
//! (`uploads`). rclone's own cache folder holds what it keeps, bounded.
//!
//! A server's password (ADR-082): rclone cannot ask, and a WebDAV mount
//! checks nothing until it is read. So an SMB, WebDAV or FTP address is
//! listed once before it is mounted: with the password the keyring keeps,
//! else none, and a refused login asks (`Ask::password`: GVfs's kept one,
//! then the window), three times at most. rclone takes the password
//! obscured (`rclone obscure -`, given it through a pipe) in its
//! environment alone (`RCLONE_SMB_PASS`…), never on a command line.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised, Server};
use crate::mounter;

/// Its type in the mount table.
pub const FSTYPE: &str = "fuse.rclone";

/// How long connecting may take: a cloud's first listing, a password.
const PATIENCE: Duration = Duration::from_secs(120);

/// rclone, through the user's own configuration, or the one a test names.
#[derive(Clone, Default)]
pub struct Rclone {
    /// The configuration rclone is given (`--config`): `None` for its own
    /// default, the user's.
    pub config: Option<std::path::PathBuf>,
    /// What rclone runs to ask its configuration's password: `None` for
    /// this executable (a test names Skwir's).
    pub askpass: Option<std::path::PathBuf>,
    /// Where the password of an encrypted configuration is kept, when the
    /// user asked (ADR-086): the desktop's keyring where `None`.
    pub keyring: Option<Arc<dyn crate::passwords::Keyring>>,
}

impl std::fmt::Debug for Rclone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rclone")
            .field("config", &self.config)
            .field("askpass", &self.askpass)
            .finish_non_exhaustive()
    }
}

impl Rclone {
    /// rclone given the configuration at `config` (a test's own).
    pub fn with_config(config: std::path::PathBuf) -> Self {
        Rclone {
            config: Some(config),
            ..Rclone::default()
        }
    }

    /// The keyring the configuration's password is kept in.
    pub fn keyring(&self) -> Arc<dyn crate::passwords::Keyring> {
        self.keyring
            .clone()
            .unwrap_or_else(|| Arc::new(crate::passwords::Desktop))
    }

    /// The program rclone runs to ask its configuration's password.
    ///
    /// # Errors
    /// When this executable cannot be found.
    pub fn askpass_program(&self) -> std::io::Result<std::path::PathBuf> {
        match &self.askpass {
            Some(program) => Ok(program.clone()),
            None => std::env::current_exe(),
        }
    }

    /// The configuration it is given, `None` for rclone's own.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }
}

/// `rclone` run with `config` where one is named.
pub(crate) fn command(rclone: &Path, config: Option<&Path>) -> Command {
    let mut command = Command::new(rclone);
    if let Some(config) = config {
        command.arg("--config").arg(config);
    }
    command
}

impl Connector for Rclone {
    fn id(&self) -> &'static str {
        "rclone"
    }

    fn title(&self) -> &'static str {
        "rclone"
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[
            Scheme::Rclone,
            Scheme::S3,
            Scheme::Sftp,
            Scheme::Smb,
            Scheme::Dav,
            Scheme::Davs,
            Scheme::Ftp,
            Scheme::Ftps,
        ]
    }

    fn unavailable(&self) -> Option<String> {
        if skwir_core::os_path::find_program("rclone").is_none() {
            return Some("rclone is not installed".to_owned());
        }
        #[cfg(target_os = "linux")]
        if ["fusermount3", "fusermount"]
            .iter()
            .all(|p| skwir_core::os_path::find_program(p).is_none())
        {
            return Some("FUSE (fusermount3) is not installed".to_owned());
        }
        // Without macFUSE or FUSE-T, rclone mounts over NFS (`nfsmount`).
        #[cfg(windows)]
        if !crate::winfsp::installed() {
            return Some("WinFsp is not installed: rclone mounts through it on Windows".to_owned());
        }
        None
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        self.connect_with(
            address,
            &crate::options::MountOptions::default(),
            ask,
            cancel,
        )
    }

    fn connect_with(
        &self,
        address: &Address,
        options: &crate::options::MountOptions,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let rclone = skwir_core::os_path::find_program("rclone")
            .ok_or_else(|| anyhow::anyhow!("rclone is not installed"))?;
        let remote = remote_of(&top)?;
        let password = match password_variable(top.scheme) {
            Some(variable) => {
                logged_in(&rclone, self.config(), &remote, &top, variable, ask, cancel)?
                    .map(|password| (variable, password))
            }
            None => None,
        };
        #[cfg(windows)]
        return self.mount_on_windows(
            &rclone,
            &remote,
            &top,
            options,
            password.as_ref(),
            ask,
            cancel,
        );
        #[cfg(not(windows))]
        self.mount_on_unix(
            &rclone,
            &remote,
            top,
            options,
            password.as_ref(),
            ask,
            cancel,
        )
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        // macOS's `nfsmount` is the system's NFS: Skwir's by its folder.
        if cfg!(target_os = "macos") && remote.fstype == "nfs" {
            return mounter::ours(self.id(), &remote.point);
        }
        if remote.fstype != FSTYPE {
            return None;
        }
        if let Some(ours) = mounter::ours(self.id(), &remote.point) {
            return Some(ours);
        }
        // Mounted by the user: `gdrive:Photos`, or `:sftp{hash}:` for an
        // address, whose parts rclone does not say.
        let source = remote.source.to_string_lossy();
        let address = source
            .split_once(':')
            .filter(|(name, _)| !name.is_empty())
            .and_then(|(name, path)| {
                let path = format!("/{}", path.trim_start_matches('/'));
                Address::rclone(name, path.as_bytes()).ok()
            });
        Some(Recognised {
            label: remote.point.file_name().map_or_else(
                || source.clone().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            address,
            ours: false,
            device: None,
        })
    }

    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        #[cfg(windows)]
        let unmounted = {
            let _ = recognised;
            unmount_on_windows(&remote.point, cancel)
        };
        #[cfg(not(windows))]
        let unmounted = mounter::unmount(&remote.point, recognised.ours, cancel);
        #[cfg(unix)]
        if unmounted.is_ok() && recognised.ours {
            quit(&remote.point, cancel);
        }
        unmounted
    }

    /// Cut, and its rclone told to quit as when let go of: cut while it
    /// waits for a server, it would wait on, and ending later unmount a
    /// mount made at its point meanwhile.
    fn force_disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        let cut = mounter::force_unmount(&remote.point);
        #[cfg(unix)]
        if recognised.ours {
            quit(&remote.point, cancel);
        }
        #[cfg(not(unix))]
        let _ = (recognised, cancel);
        cut
    }

    /// Copies within one mount Skwir made, by rclone on the server
    /// (`rclone_copy`).
    fn transfers(&self) -> Vec<Arc<dyn skwir_core::op::transfer::Transfer>> {
        #[cfg(unix)]
        {
            vec![crate::rclone_copy::shared() as Arc<dyn skwir_core::op::transfer::Transfer>]
        }
        #[cfg(not(unix))]
        {
            Vec::new()
        }
    }

    fn servers(&self, cancel: &CancelToken) -> Vec<Server> {
        let Some(rclone) = skwir_core::os_path::find_program("rclone") else {
            return Vec::new();
        };
        // An encrypted configuration is opened with the password the
        // keyring keeps for it, where the user asked (ADR-086); never asked
        // of the window: nobody asked to see it.
        let listed = |password: Option<&str>| {
            let mut command = command(&rclone, self.config());
            command
                .args(["listremotes", "--long", "--ask-password=false"])
                .env_remove("RCLONE_CONFIG_PASS");
            if let Some(password) = password {
                command.env("RCLONE_CONFIG_PASS", password);
            }
            skwir_platform::process::run_to_exit(
                &mut command,
                None,
                cancel,
                Duration::from_secs(10),
            )
        };
        let finished = match listed(None) {
            Ok(finished) if !finished.status.success() && needs_password(&finished.stderr) => {
                match self.keyring().own(crate::rclone_config::KEYRING_KEY) {
                    Some(password) => listed(Some(&password)),
                    None => Ok(finished),
                }
            }
            other => other,
        };
        match finished {
            Ok(finished) if finished.status.success() => {
                remotes(&String::from_utf8_lossy(&finished.stdout))
            }
            Ok(finished) => {
                tracing::debug!("rclone listremotes: {}", finished.stderr.trim());
                Vec::new()
            }
            Err(error) => {
                tracing::debug!("rclone listremotes: {error:#}");
                Vec::new()
            }
        }
    }
}

/// Whether what rclone said is its configuration needing a password it
/// was not given.
pub(crate) fn needs_password(said: &str) -> bool {
    said.contains("unable to decrypt configuration")
        || said.contains("not allowed to ask for password")
}

#[cfg(not(windows))]
impl Rclone {
    /// Mount `remote` (`top`'s) at a folder of Skwir's: `rclone mount
    /// --daemon` with FUSE, or `nfsmount` on macOS without it.
    #[allow(clippy::too_many_arguments)] // the parts of one connection
    fn mount_on_unix(
        &self,
        rclone: &Path,
        remote: &str,
        top: Address,
        options: &crate::options::MountOptions,
        password: Option<&(&'static str, String)>,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let point = mounter::point_for(self.id(), &top)?;
        let mut command = command(rclone, self.config());
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut args = mount_arguments(remote, &point);
        // macOS without macFUSE or FUSE-T: rclone serves the remote over NFS
        // on this machine and has the system mount it (`nfsmount`).
        #[cfg(target_os = "macos")]
        if !crate::sshfs::fuse_on_macos() {
            args[0] = "nfsmount".into();
        }
        command.args(args);
        // What the user chose for it (ADR-086): after Skwir's own, so it wins.
        command.args(crate::options::rclone_arguments(options));
        // Its remote control, the user's alone, for copies made on the
        // server (`rclone_copy`).
        #[cfg(unix)]
        if let Some(socket) = crate::rclone_copy::socket_for(&point)
            && crate::rclone_copy::socket_folder().is_ok()
        {
            // One a mount gone left: rclone would not listen there.
            let _ = std::fs::remove_file(&socket);
            let mut at = std::ffi::OsString::from("unix://");
            at.push(&socket);
            command.args(["--rc", "--rc-no-auth", "--rc-addr"]).arg(at);
        }
        if let Some((variable, password)) = password {
            command.env(variable, password);
        }
        if top.scheme == Scheme::Sftp {
            // A word each, split at spaces by rclone: names checked so.
            top.ssh_user()?;
            command.arg("--sftp-ssh").arg(ssh_command(&top));
        }
        // A configuration kept encrypted asks its password of the window.
        let attempt = mounter::attempt(cancel);
        let program = self.askpass_program()?;
        let askpass = mounter::Askpass::serve_with(program.clone(), ask, &attempt)?;
        askpass.give(&mut command);
        command
            .arg("--password-command")
            .arg(password_command(&program));
        mounter::mount(&mut command, &point, &attempt, PATIENCE).map_err(|error| {
            let error = anyhow::anyhow!("{}", words(&error.to_string()));
            mounter::ended(&attempt, cancel, error)
        })?;
        Ok(Connected {
            point,
            address: top,
        })
    }
}

#[cfg(windows)]
impl Rclone {
    /// Mount `remote` (`top`'s) through `WinFsp` in a folder of Skwir's, in
    /// the foreground with no window, its remote control on the loopback
    /// (`winfsp`): once its folder is there. What its configuration and
    /// ssh ask reaches the window, as on Unix.
    #[allow(clippy::too_many_arguments)] // the connector's, then Windows' own
    fn mount_on_windows(
        &self,
        rclone: &Path,
        remote: &str,
        top: &Address,
        options: &crate::options::MountOptions,
        password: Option<&(&'static str, String)>,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        use std::os::windows::process::CommandExt as _;
        /// `CREATE_NO_WINDOW`: no console behind it.
        const NO_WINDOW: u32 = 0x0800_0000;
        let folder = crate::connector::mounts_folder()
            .ok_or_else(|| anyhow::anyhow!("there is no folder to mount in"))?;
        mounter::make_private(&folder)?;
        skwir_core::fs::reach::mounts_in(folder.clone(), FSTYPE);
        let point = folder.join(crate::connector::point_name(self.id(), top));
        if skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) {
            anyhow::bail!("it is connected already");
        }
        // WinFsp makes the folder itself: one left empty goes first.
        let _ = std::fs::remove_dir(&point);
        let at = crate::rc::At::loopback()?;
        crate::winfsp::keep_login(&point, &at)?;
        let log = crate::winfsp::log_file(&point)
            .ok_or_else(|| anyhow::anyhow!("no folder for its log"))?;
        let _ = std::fs::remove_file(&log);
        let mut command = command(rclone, self.config());
        command
            .args(crate::winfsp::mount_arguments(remote, &point, &at, &log))
            .args(crate::options::rclone_arguments(options))
            .envs(at.serving().1)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(NO_WINDOW);
        if let Some((variable, password)) = password {
            command.env(variable, password);
        }
        if top.scheme == Scheme::Sftp {
            // Windows' own OpenSSH, its configuration and agent.
            top.ssh_user()?;
            command.arg("--sftp-ssh").arg(ssh_command(top));
        }
        // A configuration kept encrypted asks its password of the window.
        let attempt = mounter::attempt(cancel);
        let program = self.askpass_program()?;
        let askpass = mounter::Askpass::serve_with(program.clone(), ask, &attempt)?;
        askpass.give(&mut command);
        command
            .arg("--password-command")
            .arg(password_command(&program));
        let (ended_tx, ended) = crossbeam_channel::bounded(1);
        skwir_platform::process::spawn_watched(&mut command, move |status| {
            let _ = ended_tx.send(status);
        })?;
        let deadline = std::time::Instant::now() + PATIENCE;
        loop {
            skwir_core::fs::reach::changed();
            if skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) && at.answers() {
                return Ok(Connected {
                    point,
                    address: top.clone(),
                });
            }
            if ended.try_recv().is_ok() {
                let said = std::fs::read_to_string(&log).unwrap_or_default();
                crate::winfsp::forget(&point);
                return Err(mounter::ended(
                    &attempt,
                    cancel,
                    anyhow::anyhow!("{}", words(&said)),
                ));
            }
            if attempt.is_cancelled() || std::time::Instant::now() > deadline {
                let _ = crate::rc::call(
                    &at,
                    "core/quit",
                    &serde_json::json!({}),
                    &CancelToken::new(),
                );
                crate::winfsp::forget(&point);
                return Err(mounter::ended(
                    &attempt,
                    cancel,
                    anyhow::anyhow!("rclone did not mount it in time"),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Have the rclone of the mount Skwir made at `point`, let go of, end now,
/// as on Windows (`core/quit` through its remote control), and wait a
/// moment until it has. Let go of whole, it ends of itself; cut while one
/// of its calls waited for a server that does not answer
/// (`mounter::force_unmount`), it waits for that call first, minutes on
/// end, and then, ending, unmounts its point and removes its socket's
/// path: a mount made there again meanwhile would lose both.
#[cfg(unix)]
fn quit(point: &Path, cancel: &CancelToken) {
    let Some(socket) = crate::rclone_copy::socket_for(point) else {
        return;
    };
    // Ended already: nothing listens there any more.
    if crate::rclone_copy::call(&socket, "core/quit", &serde_json::json!({}), cancel).is_err() {
        return;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while socket.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Let go of the mount Skwir made at `point` on Windows, or a logon task
/// did (ADR-087): rclone asked to quit through its remote control, which
/// unmounts first. The task's login is forgotten with the mount where no
/// task is there any more (turned off while it was mounted).
#[cfg(windows)]
fn unmount_on_windows(point: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    let controls = crate::winfsp::controls_of(point);
    anyhow::ensure!(
        !controls.is_empty(),
        "Skwir did not mount it, so it cannot let go of it"
    );
    // A login kept from a mount gone is refused: the next is asked.
    let mut refused = None;
    let asked = controls.iter().any(|at| {
        crate::rc::call(at, "core/quit", &serde_json::json!({}), cancel)
            .inspect_err(|error| refused = Some(format!("{error:#}")))
            .is_ok()
    });
    if !asked {
        anyhow::bail!(
            "rclone was not reached to let go of it: {}",
            refused.unwrap_or_default()
        );
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        skwir_core::fs::reach::changed();
        if skwir_core::fs::reach::of(point).is_none_or(|r| r.point != point) {
            crate::winfsp::forget(point);
            let task = point
                .file_name()
                .and_then(|name| crate::connector::of_point_name(name.to_str()?))
                .map(|(by, address)| crate::login::id_of(by, &address));
            if task.is_none_or(|id| !crate::login::installed(&id)) {
                crate::winfsp::forget_at_login(point);
            }
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "rclone did not let go of it"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The remotes `rclone listremotes --long` names: `gdrive: drive`.
fn remotes(listed: &str) -> Vec<Server> {
    listed
        .lines()
        .filter_map(|line| {
            let (name, kind) = line.split_once(':')?;
            let name = name.trim();
            let address = Address::rclone(name, b"/").ok()?;
            let kind = kind.trim();
            Some(Server {
                label: if kind.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name} ({kind})")
                },
                address,
                kind: kind.to_owned(),
                kept: crate::servers::Kept::Tool("in rclone's configuration"),
            })
        })
        .collect()
}

/// What rclone mounts for `top`: a remote of its configuration, or one it
/// is given by its parts (a connection string, `:sftp,host=…:`), a value
/// quoted where it holds what would end it.
pub(crate) fn remote_of(top: &Address) -> anyhow::Result<String> {
    let path = String::from_utf8_lossy(&top.path)
        .trim_start_matches('/')
        .to_owned();
    let quoted = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let mut parts = Vec::new();
    let kind = match top.scheme {
        Scheme::Rclone => return Ok(format!("{}:{path}", top.host)),
        // Through the user's own AWS configuration (its credentials,
        // region and endpoint; a profile where the address names one):
        // nothing kept of it (ADR-086). The bucket is the mount's top.
        Scheme::S3 => {
            let mut remote = ":s3,provider='AWS',env_auth='true'".to_owned();
            if let Some(profile) = &top.user {
                let _ = write!(remote, ",profile={}", quoted(profile));
            }
            return Ok(format!("{remote}:{}/{path}", top.host));
        }
        // Through the user's ssh: the host and user go to it, not here.
        Scheme::Sftp => "sftp",
        Scheme::Smb => {
            parts.push(format!("host={}", quoted(top.bare_host())));
            "smb"
        }
        Scheme::Dav | Scheme::Davs => {
            let scheme = if top.scheme == Scheme::Davs {
                "https"
            } else {
                "http"
            };
            let port = top.port.map(|p| format!(":{p}")).unwrap_or_default();
            parts.push(format!(
                "url={}",
                quoted(&format!("{scheme}://{}{port}", top.host))
            ));
            "webdav"
        }
        Scheme::Ftp | Scheme::Ftps => {
            parts.push(format!("host={}", quoted(top.bare_host())));
            if top.scheme == Scheme::Ftps {
                parts.push("tls=true".to_owned());
            }
            "ftp"
        }
        Scheme::Nfs | Scheme::Afp | Scheme::Mtp | Scheme::Gphoto2 | Scheme::Afc => {
            anyhow::bail!("rclone does not open {} addresses", top.scheme.title())
        }
    };
    if top.scheme != Scheme::Sftp {
        if let Some(user) = &top.user {
            parts.push(format!("user={}", quoted(user)));
        }
        if let (Some(port), false) = (top.port, matches!(top.scheme, Scheme::Dav | Scheme::Davs)) {
            parts.push(format!("port={port}"));
        }
        if let Some(domain) = &top.domain {
            parts.push(format!("domain={}", quoted(domain)));
        }
    }
    let mut remote = format!(":{kind}");
    for part in parts {
        remote.push(',');
        remote.push_str(&part);
    }
    // WebDAV's path is the server's after its address.
    let path = match top.scheme {
        Scheme::Dav | Scheme::Davs => String::from_utf8_lossy(&top.path).into_owned(),
        _ => path,
    };
    Ok(format!("{remote}:{path}"))
}

/// The variable rclone reads the password of an address of `scheme` from,
/// obscured: `None` where it takes none (SFTP's is ssh's; a remote of its
/// configuration keeps its own).
fn password_variable(scheme: Scheme) -> Option<&'static str> {
    match scheme {
        Scheme::Smb => Some("RCLONE_SMB_PASS"),
        Scheme::Dav | Scheme::Davs => Some("RCLONE_WEBDAV_PASS"),
        Scheme::Ftp | Scheme::Ftps => Some("RCLONE_FTP_PASS"),
        Scheme::Sftp
        | Scheme::Nfs
        | Scheme::Afp
        | Scheme::Rclone
        | Scheme::S3
        | Scheme::Mtp
        | Scheme::Gphoto2
        | Scheme::Afc => None,
    }
}

/// How many times a refused login is asked again.
const TRIES: usize = 3;

/// Log in to `remote` (the address `top`'s), listing its top once: with
/// the password the keyring keeps, else none, then each refusal asking
/// (`Ask::password`), [`TRIES`] times at most. The password that served,
/// obscured for `variable`; `None` where none was needed.
fn logged_in(
    rclone: &Path,
    config: Option<&Path>,
    remote: &str,
    top: &Address,
    variable: &str,
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let title = format!("Password for {}", top.label());
    let mut password = ask.kept_password();
    let mut asked = 0;
    loop {
        let given = password
            .as_deref()
            .map(|password| obscured(rclone, password))
            .transpose()?;
        match listed(rclone, config, remote, variable, given.as_deref(), cancel) {
            Ok(()) => return Ok(given),
            Err(said) if refused(&said) && asked < TRIES => {
                asked += 1;
                let answer = ask
                    .password(&title, "Connect", cancel)
                    .ok_or_else(|| anyhow::anyhow!("you cancelled"))?;
                password = Some(answer.text);
            }
            Err(said) if refused(&said) => {
                anyhow::bail!("the server refused the login (wrong password?)")
            }
            Err(said) => anyhow::bail!("{}", words(&said)),
        }
    }
}

/// Whether rclone reaches `remote`, its top listed, given the obscured
/// `password` in `variable`; else what it said.
fn listed(
    rclone: &Path,
    config: Option<&Path>,
    remote: &str,
    variable: &str,
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<(), String> {
    let mut command = command(rclone, config);
    command
        .args([
            "lsf",
            "--max-depth",
            "1",
            "--dirs-only",
            "--ask-password=false",
        ])
        .arg(remote)
        .env_remove(variable);
    if let Some(password) = password {
        command.env(variable, password);
    }
    match skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE) {
        Ok(finished) if finished.status.success() => Ok(()),
        Ok(finished) => Err(finished.stderr),
        Err(error) => Err(format!("{error:#}")),
    }
}

/// `password` obscured as rclone reads it (`rclone obscure -`), given it
/// through a pipe: never a command line, never a file.
pub(crate) fn obscured(rclone: &Path, password: &str) -> anyhow::Result<String> {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new(rclone)
        .args(["obscure", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // A password fits a pipe's buffer: written whole before it is read.
    if let Some(mut input) = child.stdin.take() {
        input.write_all(password.as_bytes())?;
        input.write_all(b"\n")?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(out.status.success(), "rclone did not obscure the password");
    let text = String::from_utf8(out.stdout)?;
    Ok(text.trim().to_owned())
}

/// Whether what rclone said is the server refusing the login, or its
/// wanting a password it was not given.
fn refused(said: &str) -> bool {
    [
        "401 Unauthorized",
        "530 ",
        "not logged in",
        "Login incorrect",
        "LOGON_FAILURE",
        "logon failure",
        "is it obscured",
        "attempted logon is invalid",
    ]
    .iter()
    .any(|sign| said.contains(sign))
}

/// The ssh rclone runs for an SFTP address: the user's own, its
/// configuration and agent, a server gone told within a minute.
fn ssh_command(top: &Address) -> String {
    let mut ssh = String::from("ssh -o ServerAliveInterval=15 -o ServerAliveCountMax=3");
    if let Some(port) = top.port {
        let _ = write!(ssh, " -p {port}");
    }
    ssh.push(' ');
    if let Some(user) = &top.user {
        ssh.push_str(user);
        ssh.push('@');
    }
    ssh.push_str(top.bare_host());
    ssh
}

/// `rclone mount`'s arguments for `remote` at `point`.
pub(crate) fn mount_arguments(remote: &str, point: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec!["mount".into(), remote.into(), point.into()];
    args.extend(
        [
            "--daemon",
            "--daemon-wait",
            "60s",
            "--vfs-cache-mode",
            "minimal",
            // What a cache keeps (`writes`, `full`): an hour unread, a
            // gigabyte, and never the disk's last two. A file open or
            // waiting to be sent stays whatever the bounds.
            "--vfs-cache-max-age",
            "1h",
            "--vfs-cache-max-size",
            "1G",
            "--vfs-cache-min-free-space",
            "2G",
            // What the server changed is seen within a minute where it
            // says (and on F5 and focus everywhere).
            "--dir-cache-time",
            "1m",
            "--log-level",
            "ERROR",
        ]
        .map(std::ffi::OsString::from),
    );
    args
}

/// `program` as rclone's `--password-command` reads it: a list of words
/// split at spaces, CSV's way, so a path holding a space (a profile
/// `C:\Users\Jean Dupont`) is one word in double quotes, a quote in it
/// doubled.
pub(crate) fn password_command(program: &Path) -> std::ffi::OsString {
    let mut quoted = vec![b'"'];
    for &byte in program.as_os_str().as_encoded_bytes() {
        if byte == b'"' {
            quoted.push(b'"');
        }
        quoted.push(byte);
    }
    quoted.push(b'"');
    skwir_core::os_path::from_bytes(quoted)
}

/// What rclone says on failing, in the user's words where it can be told.
pub(crate) fn words(said: &str) -> String {
    let said = said.trim();
    let line = said
        .lines()
        .rev()
        .find(|l| l.contains("CRITICAL") || l.contains("ERROR"))
        .unwrap_or_else(|| said.lines().last().unwrap_or(said));
    // `2026/10/21 00:00:00 CRITICAL: Failed to create file system for …: …`
    let line = line.split_once(": ").map_or(line, |(head, rest)| {
        if head.contains("CRITICAL") || head.contains("ERROR") {
            rest
        } else {
            line
        }
    });
    if line.contains("didn't find section in config file") {
        "rclone's configuration names no such remote".to_owned()
    } else if line.contains("connection refused") {
        "the server refused the connection".to_owned()
    } else if line.contains("no such host") {
        "the server's name was not found".to_owned()
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A password command whose path holds spaces and a quote is run
    /// whole: rclone splits the flag at spaces, CSV's way. Against the
    /// rclone installed, a script standing in for Skwir; skipped, saying
    /// why, with none.
    #[cfg(unix)]
    #[test]
    fn a_password_command_is_one_word_whatever_its_path() {
        assert_eq!(
            password_command(Path::new("/a b/say \"hi\"")),
            std::ffi::OsString::from("\"/a b/say \"\"hi\"\"\"")
        );
        let Some(rclone) = skwir_core::os_path::find_program("rclone") else {
            eprintln!("skipped: rclone is not installed");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let config = folder.path().join("rclone.conf");
        std::fs::write(&config, "[kept]\ntype = memory\n").unwrap();
        let program = folder.path().join("Jean Dupont").join("say \"it\"");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "#!/bin/sh\necho the-password\n").unwrap();
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let run = |args: &[&str]| {
            let mut command = super::command(&rclone, Some(&config));
            command
                .args(args)
                .arg("--password-command")
                .arg(password_command(&program))
                .env_remove("RCLONE_CONFIG_PASS");
            command.output().unwrap()
        };
        let set = run(&["config", "encryption", "set"]);
        assert!(set.status.success(), "{set:?}");
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("RCLONE_ENCRYPT_V0"),
            "encrypted"
        );
        let listed = run(&["listremotes"]);
        assert!(listed.status.success(), "{listed:?}");
        assert_eq!(String::from_utf8_lossy(&listed.stdout).trim(), "kept:");
    }

    #[test]
    fn rclone_is_given_an_address_by_its_parts() {
        let top = |text: &str| Address::parse(text).unwrap().top();
        assert_eq!(
            remote_of(&top("rclone://gdrive/Photos")).unwrap(),
            "gdrive:"
        );
        assert_eq!(
            remote_of(&top("smb://WORK;me@nas/photos/2024")).unwrap(),
            ":smb,host='nas',user='me',domain='WORK':photos/"
        );
        assert_eq!(
            remote_of(&top("davs://cloud.example.com:8443/remote.php/webdav")).unwrap(),
            ":webdav,url='https://cloud.example.com:8443':/remote.php/webdav/"
        );
        assert_eq!(remote_of(&top("sftp://me@h:2222/x")).unwrap(), ":sftp:");
        assert_eq!(
            ssh_command(&top("sftp://me@h:2222/x")),
            "ssh -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -p 2222 me@h"
        );
        assert!(remote_of(&top("nfs://h/export")).is_err());
        // S3 through the user's AWS configuration, its profile named.
        assert_eq!(
            remote_of(&top("s3://bucket-one.example/a/b")).unwrap(),
            ":s3,provider='AWS',env_auth='true':bucket-one.example/"
        );
        assert_eq!(
            remote_of(&top("s3://work@bucket-one/")).unwrap(),
            ":s3,provider='AWS',env_auth='true',profile='work':bucket-one/"
        );
        assert_eq!(
            remote_of(&top("ftp://o'brien@h/")).unwrap(),
            ":ftp,host='h',user='o''brien':",
            "a quote doubled"
        );
    }

    #[test]
    fn a_refused_login_is_told_from_another_failure() {
        assert!(refused(
            "couldn't list files: 401 Unauthorized: 401 Unauthorized"
        ));
        assert!(refused(
            "failed to make FTP connection to \"h:21\": 530 \"Incorrect password, not logged in\""
        ));
        assert!(refused(
            "NewFS decrypt password: input too short when revealing password - is it obscured?"
        ));
        assert!(refused(
            "couldn't connect SMB: response error: The attempted logon is invalid. This is either due to a bad username or authentication information."
        ));
        assert!(!refused(
            "dial tcp 127.0.0.1:1: connect: connection refused"
        ));
        assert_eq!(password_variable(Scheme::Davs), Some("RCLONE_WEBDAV_PASS"));
        assert_eq!(password_variable(Scheme::Sftp), None, "ssh's");
    }

    #[test]
    fn the_configurations_remotes_are_servers() {
        let servers = remotes("gdrive:     drive\nnas:        sftp\n");
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].address.to_string(), "rclone://gdrive/");
        assert_eq!(servers[0].label, "gdrive (drive)");
    }

    #[test]
    fn rclone_mounts_minimal_unless_a_cache_is_chosen_and_bounds_what_it_keeps() {
        let args = mount_arguments("gdrive:", Path::new("/m"));
        let args: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let value = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].clone()
        };
        // The options' cache, Automatic's included, comes later and wins.
        assert_eq!(value("--vfs-cache-mode"), "minimal");
        assert_eq!(value("--vfs-cache-max-age"), "1h");
        assert_eq!(value("--vfs-cache-max-size"), "1G");
        assert_eq!(value("--vfs-cache-min-free-space"), "2G");
        assert_eq!(args[..3], ["mount", "gdrive:", "/m"]);
    }
}
