//! Key login for SFTP set up from Skwir: the keys offered (those the ssh
//! agent holds, which `KeePassXC`, gpg-agent or a security key fill; those
//! ssh tries by default; a new one made here), their public halves added
//! to the server's authorized keys, then a login by key tried.
//!
//! What ssh asks while the keys go up (the server's password, a host never
//! seen) reaches the window through the `skwir` executable as
//! `SSH_ASKPASS` (ADR-077). A new key's passphrase is given to ssh-keygen
//! so too, never on its command line, which every user of the machine can
//! read. Only public halves leave the machine.
//!
//! The keys go up over one login: the first ssh is the master of a control
//! socket of the runtime folder (`Master`), and a POSIX shell on the
//! server adds those it lacks (`ADD`). A server that runs no command (an
//! account held to SFTP, rclone's server, Windows' `cmd`) takes them
//! through sftp over the same connection: its authorized keys read, those
//! missing added, the file put back by a rename. Not ssh-copy-id: it makes
//! a scratch folder in `$HOME/.ssh` whatever it is asked; it adds again
//! what is there unless it first logs in with each key, which for keys
//! given in a file needs their private halves beside them; it takes the
//! SFTP way only when told beforehand; and macOS and Windows have none.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_platform::process;

use crate::address::{Address, Scheme};
use crate::connector::{self, Ask};
use crate::mounter;

/// How long setting up may take, a password typed included.
const PATIENCE: Duration = Duration::from_secs(300);

/// How long a login asking nothing may take.
const QUIET: Duration = Duration::from_secs(60);

/// How long a program of this machine may take: the agent, ssh-keygen.
const NEAR: Duration = Duration::from_secs(30);

/// The files ssh tries by default, in its order (`ssh -i`): `id_xmss` and
/// `id_dsa` for its releases before 10.
const DEFAULT_KEYS: &[&str] = &[
    "id_rsa",
    "id_ecdsa",
    "id_ecdsa_sk",
    "id_ed25519",
    "id_ed25519_sk",
    "id_mldsa44_ed25519",
    "id_xmss",
    "id_dsa",
];

/// The key Skwir makes: ed25519, by the name ssh tries.
const NEW_KEY: &str = "id_ed25519";

/// The most read of a public key's file.
const PUBLIC_MOST: u64 = 64 * 1024;

/// The most read of a server's authorized keys.
const AUTHORIZED_MOST: u64 = 1024 * 1024;

/// What a POSIX shell on the server runs, the keys on its input: each one
/// whose blob no line holds is added to the authorized keys, made with
/// only the user allowed in where missing (where ssh-copy-id puts them:
/// `OpenWrt`'s root keeps them in Dropbear's folder, Haiku in its own). It
/// says it runs, then how many it added. One line holding no `'` and no
/// `!`: it goes in single quotes through the login shell, whatever that is
/// (tcsh, fish).
const ADD: &str = concat!(
    "set -f; echo skwir-keys; cd || exit 1; umask 077; ",
    "f=.ssh/authorized_keys; ",
    "[ -f /etc/openwrt_release ] && ",
    "{ [ \"$LOGNAME\" = root ] || [ \"$(id -u)\" = 0 ]; } && ",
    "f=/etc/dropbear/authorized_keys; ",
    "[ \"$(uname -s)\" = Haiku ] && f=config/settings/ssh/authorized_keys; ",
    "d=$(dirname \"$f\"); mkdir -p \"$d\" || exit 1; ",
    "[ -z \"$(tail -c 1 \"$f\" 2>/dev/null)\" ] || echo >> \"$f\" || exit 1; ",
    "n=0; while IFS= read -r k; do set -- $k; [ -n \"$2\" ] || continue; ",
    "grep -qF -e \"$2\" \"$f\" 2>/dev/null && continue; ",
    "printf \"%s\\n\" \"$k\" >> \"$f\" || exit 1; n=$((n + 1)); done; ",
    "if type restorecon >/dev/null 2>&1; then restorecon -F \"$d\" \"$f\"; fi; ",
    "echo skwir-keys-added $n",
);

/// What [`ADD`] says as it starts, and as it ends with how many it added.
const ADDING: &str = "skwir-keys";
const ADDED: &str = "skwir-keys-added ";

/// A public key, as ssh writes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// Its line, what the server is given: `ssh-ed25519 AAAA… comment`.
    pub line: String,
    /// Its type: `ssh-ed25519`, `sk-ssh-ed25519@openssh.com` (a security
    /// key's).
    pub kind: String,
    /// What it says of itself, often whose it is (`me@laptop`); may be
    /// empty.
    pub comment: String,
    /// `SHA256:…`, as ssh says it; empty where ssh-keygen could not.
    pub fingerprint: String,
    /// The file of its private half as ssh is given it
    /// (`~/.ssh/id_ed25519`); `None` for the agent's.
    pub file: Option<PathBuf>,
}

impl Key {
    /// The key `line` says, as `ssh-add -L` and a `.pub` file write it;
    /// `None` for anything else, a certificate too (its key is listed
    /// beside it). Its fingerprint is left to be found.
    pub fn parse(line: &str) -> Option<Key> {
        let (kind, rest) = line.trim().split_once(char::is_whitespace)?;
        let rest = rest.trim_start();
        let (blob, comment) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let kind_ok = kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '@' | '.' | '_'));
        let blob_ok = blob.len() >= 16
            && blob
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
        if !kind_ok || !blob_ok || kind.contains("-cert-") {
            return None;
        }
        let comment: String = comment
            .trim()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let line = if comment.is_empty() {
            format!("{kind} {blob}")
        } else {
            format!("{kind} {blob} {comment}")
        };
        Some(Key {
            line,
            kind: kind.to_owned(),
            comment,
            fingerprint: String::new(),
            file: None,
        })
    }

    /// Whether `other` is the same key, whatever each says of itself: one
    /// of the default files the agent holds too.
    pub fn same(&self, other: &Key) -> bool {
        self.blob() == other.blob()
    }

    /// Its key itself, in base64: what a line of authorized keys holding it
    /// holds.
    fn blob(&self) -> &str {
        self.line.split(' ').nth(1).unwrap_or_default()
    }

    /// With its fingerprint, as ssh-keygen says it.
    fn found(mut self, cancel: &CancelToken) -> Key {
        self.fingerprint = fingerprint(&self.line, cancel);
        self
    }
}

/// How ssh is run to reach a server: as the user's own configuration,
/// agent and known hosts say, unless a test names its own (as
/// [`Sshfs`](crate::sshfs::Sshfs)'s fields do).
#[derive(Clone, Debug, Default)]
pub struct Ssh {
    /// The program ssh asks through: the `skwir` executable running, unless
    /// a test names it.
    pub askpass: Option<PathBuf>,
    /// Options given after Skwir's (`UserKnownHostsFile=…`): a test's.
    pub options: Vec<String>,
    /// A configuration file in place of the user's (`-F`): a test's.
    pub config: Option<PathBuf>,
}

impl Ssh {
    /// As `sshfs` runs it: the same files, so a server it reaches is
    /// reached so here.
    pub fn of(sshfs: &crate::sshfs::Sshfs) -> Ssh {
        Ssh {
            askpass: sshfs.askpass.clone(),
            options: sshfs.ssh_options.clone(),
            config: sshfs.ssh_config.clone(),
        }
    }

    fn askpass(&self) -> anyhow::Result<PathBuf> {
        match &self.askpass {
            Some(program) => Ok(program.clone()),
            None => Ok(std::env::current_exe()?),
        }
    }

    /// `program` (ssh or sftp) given Skwir's options `ours` first: ssh
    /// keeps an option's first value, so they hold over `options` and the
    /// user's configuration.
    fn command(&self, program: &Path, ours: &[String]) -> Command {
        let mut command = Command::new(program);
        command.env("LC_ALL", "C");
        for option in ours.iter().chain(&self.options) {
            command.arg("-o").arg(option);
        }
        if let Some(config) = &self.config {
            command.arg("-F").arg(config);
        }
        command
    }
}

/// An SFTP server as ssh and sftp are given it.
struct Server {
    /// `me@host`, an IPv6 host bare, for ssh.
    ssh: String,
    /// `me@[fe80::1]`, for sftp, which reads a `:` as the start of a path.
    sftp: String,
    port: Option<u16>,
}

impl Server {
    /// `address`'s server.
    ///
    /// # Errors
    /// When it is no SFTP address, or its user's name is no name ssh takes.
    fn of(address: &Address) -> anyhow::Result<Server> {
        anyhow::ensure!(
            address.scheme == Scheme::Sftp,
            "key login is set up for SFTP servers"
        );
        address.ssh_user()?;
        let user = address
            .user
            .as_deref()
            .map(|user| format!("{user}@"))
            .unwrap_or_default();
        Ok(Server {
            ssh: format!("{user}{}", address.bare_host()),
            sftp: format!("{user}{}", address.host),
            port: address.port,
        })
    }

    /// ssh's arguments after its options: the port, the server.
    fn for_ssh(&self, command: &mut Command) {
        if let Some(port) = self.port {
            command.arg("-p").arg(port.to_string());
        }
        command.arg(&self.ssh);
    }

    /// sftp's.
    fn for_sftp(&self, command: &mut Command) {
        if let Some(port) = self.port {
            command.arg("-P").arg(port.to_string());
        }
        command.arg(&self.sftp);
    }
}

/// Options for every ssh run here: no forwarding of the agent, the display
/// or a port to a server being set up, no terminal, the command Skwir
/// gives run (never the configuration's own, here or there), a server
/// that does not answer given up on.
fn quiet_session() -> Vec<String> {
    [
        "ForwardAgent=no",
        "ForwardX11=no",
        "ClearAllForwardings=yes",
        "RequestTTY=no",
        "RemoteCommand=none",
        "PermitLocalCommand=no",
        "ConnectTimeout=30",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// One login shared by the ssh and sftp run after it, through a control
/// socket of the runtime folder: the first ssh given [`Master::options`]
/// logs in and goes on serving in the background (`ControlPersist`). Told
/// to end when dropped; else it ends on its own a minute after the last.
struct Master<'a> {
    socket: PathBuf,
    program: PathBuf,
    ssh: &'a Ssh,
    server: &'a Server,
}

impl<'a> Master<'a> {
    /// A socket of its own, none yet listening there.
    fn new(program: &Path, ssh: &'a Ssh, server: &'a Server) -> anyhow::Result<Master<'a>> {
        let socket = private_folder()?.join(unique("keys"));
        let _ = std::fs::remove_file(&socket);
        Ok(Master {
            socket,
            program: program.to_owned(),
            ssh,
            server,
        })
    }

    /// Its socket for ssh's options: quoted, a `%` written as ssh reads one.
    fn path(&self) -> String {
        let path = self.socket.display().to_string().replace('%', "%%");
        format!("ControlPath=\"{path}\"")
    }

    /// For the first ssh: the master, serving after its command ends.
    fn options(&self) -> Vec<String> {
        vec![
            "ControlMaster=yes".to_owned(),
            self.path(),
            "ControlPersist=60".to_owned(),
        ]
    }

    /// For the others: through it, never a master of their own.
    fn through(&self) -> Vec<String> {
        vec!["ControlMaster=no".to_owned(), self.path()]
    }

    /// `ssh -O what`, asked of it.
    fn control(&self, what: &str) -> bool {
        let mut command = self.ssh.command(&self.program, &self.through());
        command.arg("-O").arg(what);
        self.server.for_ssh(&mut command);
        process::run_to_exit(&mut command, None, &CancelToken::new(), NEAR)
            .is_ok_and(|finished| finished.status.success())
    }

    /// Whether it logged in, and serves.
    fn running(&self) -> bool {
        self.control("check")
    }
}

impl Drop for Master<'_> {
    fn drop(&mut self) {
        if self.socket.exists() {
            self.control("exit");
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// The folder of the runtime folder that what one connection uses for
/// its while is kept in (askpass's socket, a control socket), private to
/// the user.
fn private_folder() -> anyhow::Result<PathBuf> {
    let folder = connector::mounts_folder()
        .and_then(|folder| folder.parent().map(Path::to_path_buf))
        .ok_or_else(|| anyhow::anyhow!("there is no runtime folder"))?;
    mounter::make_private(&folder)?;
    Ok(folder)
}

/// `prefix` and a number no other call of this process says.
fn unique(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// What ssh-keygen says of `line`: `SHA256:…`; empty where it cannot.
fn fingerprint(line: &str, cancel: &CancelToken) -> String {
    let Some(keygen) = skwir_core::os_path::find_program("ssh-keygen") else {
        return String::new();
    };
    let mut command = Command::new(keygen);
    command.args(["-l", "-f", "-"]);
    let input = format!("{line}\n");
    process::run_to_exit(&mut command, Some(input.as_bytes()), cancel, NEAR)
        .ok()
        .filter(|finished| finished.status.success())
        .and_then(|finished| {
            String::from_utf8_lossy(&finished.stdout)
                .split_whitespace()
                .nth(1)
                .filter(|fingerprint| fingerprint.contains(':'))
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// The user's `~/.ssh`, where ssh keeps its keys.
pub fn ssh_dir() -> Option<PathBuf> {
    skwir_core::os_path::home().map(|home| home.join(".ssh"))
}

/// The keys the ssh agent holds (`ssh-add -L`): `KeePassXC`'s, gpg-agent's,
/// a security key's, those `ssh-add` was given. `socket` names the agent
/// (`SSH_AUTH_SOCK`), the session's where `None`. None where no agent
/// answers, or it holds no key.
pub fn agent_keys(socket: Option<&Path>, cancel: &CancelToken) -> Vec<Key> {
    let Some(program) = skwir_core::os_path::find_program("ssh-add") else {
        return Vec::new();
    };
    let mut command = Command::new(program);
    command.arg("-L");
    if let Some(socket) = socket {
        command.env("SSH_AUTH_SOCK", socket);
    }
    // 1: it holds none ("The agent has no identities"); 2: no agent.
    let Ok(finished) = process::run_to_exit(&mut command, None, cancel, NEAR) else {
        return Vec::new();
    };
    if !finished.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&finished.stdout)
        .lines()
        .filter_map(Key::parse)
        .map(|key| key.found(cancel))
        .collect()
}

/// The keys ssh tries by default from `ssh_dir` (the user's `~/.ssh`,
/// [`ssh_dir`]), in its order: those whose public half is there beside
/// the private one (`id_ed25519.pub`).
pub fn default_keys(ssh_dir: &Path, cancel: &CancelToken) -> Vec<Key> {
    DEFAULT_KEYS
        .iter()
        .filter_map(|name| {
            let public = ssh_dir.join(format!("{name}.pub"));
            let text = skwir_core::fs::file::read_regular_text(&public, PUBLIC_MOST).ok()?;
            let key = text.lines().find_map(Key::parse)?;
            Some(Key {
                file: Some(ssh_dir.join(name)),
                ..key.found(cancel)
            })
        })
        .collect()
}

/// Whether [`new_key`] can make a key in `ssh_dir`: neither its file nor
/// its public half is there (it never writes over one).
pub(crate) fn makes_new_key(ssh_dir: &Path) -> bool {
    [NEW_KEY.to_owned(), format!("{NEW_KEY}.pub")]
        .iter()
        .all(|name| std::fs::symlink_metadata(ssh_dir.join(name)).is_err())
}

/// A new ed25519 key in `ssh_dir` (made with only the user allowed in
/// where missing), by the name ssh tries by default, never in place of one
/// there. `passphrase` encrypts it, nothing where empty: ssh-keygen asks it
/// twice through `ssh`'s askpass and is answered here, never given it on
/// its command line.
///
/// # Errors
/// When a key is there by that name, or ssh-keygen failed.
pub fn new_key(
    ssh_dir: &Path,
    comment: &str,
    passphrase: &str,
    ssh: &Ssh,
    cancel: &CancelToken,
) -> anyhow::Result<Key> {
    anyhow::ensure!(
        !comment.chars().any(char::is_control),
        "a key's comment is one line"
    );
    let program = process::required("ssh-keygen")?;
    mounter::make_private(ssh_dir)?;
    let file = ssh_dir.join(NEW_KEY);
    let public = ssh_dir.join(format!("{NEW_KEY}.pub"));
    for path in [&file, &public] {
        anyhow::ensure!(
            std::fs::symlink_metadata(path).is_err(),
            "there is a key at {} already",
            path.display()
        );
    }
    // Its two questions answered with the passphrase, nothing else.
    let answered = Arc::new(AtomicUsize::new(0));
    let asking = if passphrase.is_empty() {
        None
    } else {
        let (answer, count) = (passphrase.to_owned(), Arc::clone(&answered));
        let server = skwir_platform::askpass::Server::start(&private_folder()?, move |question| {
            (question.kind == skwir_platform::askpass::Kind::Secret
                && count.fetch_add(1, Ordering::SeqCst) < 2)
                .then(|| answer.clone())
        })?;
        Some((ssh.askpass()?, server))
    };
    let mut command = keygen(
        &program,
        &file,
        comment,
        passphrase,
        asking
            .as_ref()
            .map(|(program, server)| (program.as_path(), server.path())),
    );
    let made = process::run_to_exit(&mut command, None, cancel, NEAR);
    drop(asking);
    let removed = || {
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&public);
    };
    let finished = made.inspect_err(|_| removed())?;
    if !finished.status.success() {
        removed();
        anyhow::bail!("ssh-keygen failed: {}", finished.stderr);
    }
    // An askpass that never ran would have given ssh-keygen an empty
    // passphrase: a key left unencrypted is no key the user asked for.
    if !passphrase.is_empty() && answered.load(Ordering::SeqCst) != 2 {
        removed();
        anyhow::bail!("ssh-keygen was not given the passphrase");
    }
    let text = skwir_core::fs::file::read_regular_text(&public, PUBLIC_MOST)?;
    let key = text
        .lines()
        .find_map(Key::parse)
        .ok_or_else(|| anyhow::anyhow!("ssh-keygen wrote no public key"))?;
    Ok(Key {
        file: Some(file),
        ..key.found(cancel)
    })
}

/// ssh-keygen making `file`: with an empty passphrase, said so (`-N ""`
/// says no secret); else asking through `asking` (the askpass program and
/// the socket it answers on), the passphrase never among its arguments
/// nor its environment.
fn keygen(
    program: &Path,
    file: &Path,
    comment: &str,
    passphrase: &str,
    asking: Option<(&Path, &Path)>,
) -> Command {
    let mut command = Command::new(program);
    command
        .env("LC_ALL", "C")
        .args(["-q", "-t", "ed25519", "-C", comment, "-f"])
        .arg(file);
    match asking {
        Some((askpass, socket)) if !passphrase.is_empty() => {
            command
                .env("SSH_ASKPASS", askpass)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(skwir_platform::askpass::SOCKET_VAR, socket);
        }
        _ => {
            command.args(["-N", ""]);
        }
    }
    command
}

/// Add `keys` (their public halves) to the authorized keys of `address`'s
/// user on its server, those it has already left as they are. ssh asks
/// what it must through `ask`: the server's password once, a host never
/// seen. How many it added.
///
/// # Errors
/// What ssh or the server said, in the user's words where it can be told;
/// "you cancelled" where a question was refused.
pub fn install(
    address: &Address,
    keys: &[Key],
    ssh: &Ssh,
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> anyhow::Result<usize> {
    anyhow::ensure!(!keys.is_empty(), "no key was chosen");
    let server = Server::of(address)?;
    let program = process::required("ssh")?;
    let attempt = mounter::attempt(cancel);
    let askpass = mounter::Askpass::serve_with(ssh.askpass()?, ask, &attempt)?;
    let master = Master::new(&program, ssh, &server)?;
    let mut command = ssh.command(&program, &[master.options(), quiet_session()].concat());
    server.for_ssh(&mut command);
    command.arg(format!("exec sh -c '{ADD}'"));
    askpass.give(&mut command);
    let lines: String = keys
        .iter()
        .flat_map(|key| [key.line.as_str(), "\n"])
        .collect();
    let finished = process::run_to_exit(&mut command, Some(lines.as_bytes()), &attempt, PATIENCE)
        .map_err(|error| mounter::ended(&attempt, cancel, error))?;
    let said = String::from_utf8_lossy(&finished.stdout);
    if let Some(added) = said.lines().find_map(|line| line.strip_prefix(ADDED)) {
        return Ok(added.trim().parse().unwrap_or(0));
    }
    if said.lines().any(|line| line == ADDING) {
        anyhow::bail!(
            "the server could not add the keys: {}",
            finished
                .stderr
                .lines()
                .last()
                .unwrap_or("it did not say why")
        );
    }
    if !master.running() {
        let error = anyhow::anyhow!("{}", crate::sshfs::words(&finished.stderr));
        return Err(mounter::ended(&attempt, cancel, error));
    }
    // Logged in to a server that runs no shell: through sftp.
    by_sftp(&master, keys, &attempt).map_err(|error| mounter::ended(&attempt, cancel, error))
}

/// [`install`] through sftp over `master`'s login: the authorized keys
/// read, those of `keys` missing added, the file put beside them and
/// renamed over them, so a connection lost on the way leaves them whole.
fn by_sftp(master: &Master<'_>, keys: &[Key], cancel: &CancelToken) -> anyhow::Result<usize> {
    let program = process::required("sftp")?;
    let folder = private_folder()?;
    let name = unique("keys");
    let (read, written) = (format!("{name}.server"), format!("{name}.new"));
    let _gone = Gone(vec![folder.join(&read), folder.join(&written)]);
    let session = |batch: String| -> anyhow::Result<process::Finished> {
        let mut command = master.ssh.command(&program, &master.through());
        command.current_dir(&folder).args(["-b", "-"]);
        master.server.for_sftp(&mut command);
        process::run_to_exit(&mut command, Some(batch.as_bytes()), cancel, PATIENCE)
    };
    let got = session(format!("get .ssh/authorized_keys {read}\n"))?;
    let had = if got.status.success() {
        skwir_core::fs::file::read_regular_text(&folder.join(&read), AUTHORIZED_MOST)?
    } else if got.stderr.contains("not found") {
        String::new()
    } else {
        anyhow::bail!(
            "the server's authorized keys could not be read: {}",
            got.stderr
        );
    };
    let (text, added) = merged(&had, keys);
    if added == 0 {
        return Ok(0);
    }
    private_file(&folder.join(&written), &text)?;
    let staged = format!(".ssh/authorized_keys.{name}");
    let made = if had.is_empty() {
        "-mkdir .ssh\n-chmod 700 .ssh\n"
    } else {
        ""
    };
    let batch = format!(
        "{made}put {written} {staged}\n-chmod 600 {staged}\nrename {staged} .ssh/authorized_keys\n"
    );
    let put = session(batch)?;
    if !put.status.success() {
        let _ = session(format!("-rm {staged}\n"));
        anyhow::bail!("the server refused the keys: {}", put.stderr);
    }
    Ok(added)
}

/// A new file at `path` holding `text`, only the user allowed in: sftp
/// gives the server its mode.
fn private_file(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

/// Files removed when dropped.
struct Gone(Vec<PathBuf>);

impl Drop for Gone {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `had` (a server's authorized keys) with the keys of `keys` it lacks
/// added after, and how many: a key is there when a line holds its blob,
/// whatever its options or comment, as [`ADD`]'s `grep -F` finds it.
fn merged(had: &str, keys: &[Key]) -> (String, usize) {
    let mut text = had.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let mut added = 0;
    for key in keys {
        if !text.contains(key.blob()) {
            text.push_str(&key.line);
            text.push('\n');
            added += 1;
        }
    }
    (text, added)
}

/// Whether `address`'s server lets its user in by key, as ssh offers keys
/// to it (its configuration's, the agent's, the default files): never by
/// password. With `ask`, a key's passphrase and a host never seen are
/// asked of it; without, nothing is asked, and a key whose passphrase is
/// needed does not count.
pub fn logs_in(
    address: &Address,
    ssh: &Ssh,
    ask: Option<&Arc<dyn Ask>>,
    cancel: &CancelToken,
) -> bool {
    let (Ok(server), Some(program)) = (
        Server::of(address),
        skwir_core::os_path::find_program("ssh"),
    ) else {
        return false;
    };
    let attempt = mounter::attempt(cancel);
    // Where the window cannot be asked (no askpass made), nothing is.
    let askpass = ask.and_then(|ask| {
        ssh.askpass()
            .and_then(|program| mounter::Askpass::serve_with(program, ask, &attempt))
            .ok()
    });
    let mut ours = vec!["PreferredAuthentications=publickey".to_owned()];
    if askpass.is_none() {
        ours.push("BatchMode=yes".to_owned());
    }
    ours.extend(quiet_session());
    // Logged in is the master serving, whatever the server made of `true`:
    // rclone's runs no command, an account held to SFTP runs its own.
    // Windows' ssh makes no master: there, `true` ran.
    #[cfg(unix)]
    let Ok(master) = Master::new(&program, ssh, &server) else {
        return false;
    };
    #[cfg(unix)]
    ours.extend(master.options());
    let mut command = ssh.command(&program, &ours);
    server.for_ssh(&mut command);
    command.arg("true");
    if let Some(askpass) = &askpass {
        askpass.give(&mut command);
    }
    let patience = if askpass.is_some() { PATIENCE } else { QUIET };
    let Ok(finished) = process::run_to_exit(&mut command, None, &attempt, patience) else {
        return false;
    };
    #[cfg(unix)]
    {
        let _ = finished;
        master.running()
    }
    #[cfg(not(unix))]
    {
        finished.status.success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIA9BzmjwrOpO5Mw3V0cOf9RSovfzeb2QXvl6e/zPICxp";

    #[test]
    fn a_public_key_is_read_as_ssh_writes_it() {
        let key = Key::parse(&format!("ssh-ed25519 {ED}  me@laptop (work)\r")).unwrap();
        assert_eq!(key.kind, "ssh-ed25519");
        assert_eq!(key.comment, "me@laptop (work)");
        assert_eq!(key.line, format!("ssh-ed25519 {ED} me@laptop (work)"));
        let bare = Key::parse(&format!("sk-ssh-ed25519@openssh.com {ED}")).unwrap();
        assert_eq!(bare.line, format!("sk-ssh-ed25519@openssh.com {ED}"));
        assert!(bare.same(&key), "the same key, whatever its comment");
        assert!(Key::parse(&format!("ssh-ed25519-cert-v01@openssh.com {ED} c")).is_none());
        assert!(Key::parse("The agent has no identities.").is_none());
        assert!(Key::parse(&format!("ssh-ed25519 {ED};rm")).is_none());
        assert!(Key::parse("").is_none());
    }

    #[test]
    fn keys_a_server_has_are_not_added_again() {
        let key = |comment: &str| Key::parse(&format!("ssh-ed25519 {ED} {comment}")).unwrap();
        let other = Key::parse(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINZpqb9Oa0V1xLwK0d6m9jMUx0nBdnf6UJtP8w5fPq7a b",
        )
        .unwrap();
        // Held with options and another comment: there.
        let had = format!("no-pty ssh-ed25519 {ED} elsewhere");
        let (text, added) = merged(&had, &[key("a"), other.clone(), other.clone()]);
        assert_eq!(added, 1);
        assert_eq!(text, format!("{had}\n{}\n", other.line));
        let (text, added) = merged("", &[key("a")]);
        assert_eq!((text, added), (format!("{}\n", key("a").line), 1));
    }

    #[test]
    fn the_shell_s_script_goes_whole_through_any_login_shell() {
        assert!(!ADD.contains('\''), "it is single-quoted");
        assert!(!ADD.contains('!'), "tcsh expands it even so");
        assert!(!ADD.contains('\n'), "tcsh takes one line");
        assert!(ADD.starts_with("set -f; echo skwir-keys;"));
    }

    #[test]
    fn a_passphrase_is_never_on_ssh_keygen_s_command_line() {
        let passphrase = "a long passphrase";
        let command = keygen(
            Path::new("ssh-keygen"),
            Path::new("/k/id_ed25519"),
            "me@laptop",
            passphrase,
            Some((Path::new("/bin/skwir"), Path::new("/run/askpass.sock"))),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-q",
                "-t",
                "ed25519",
                "-C",
                "me@laptop",
                "-f",
                "/k/id_ed25519"
            ]
        );
        let envs: Vec<(String, String)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            })
            .collect();
        assert!(envs.iter().all(|(_, v)| !v.contains(passphrase)));
        assert!(
            envs.contains(&("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned())),
            "{envs:?}"
        );
        // None: said so, which tells no secret.
        let none = keygen(
            Path::new("ssh-keygen"),
            Path::new("/k/id_ed25519"),
            "",
            "",
            None,
        );
        let args: Vec<_> = none.get_args().collect();
        assert_eq!(args[args.len() - 2..], ["-N", ""]);
    }

    #[test]
    fn a_master_s_socket_is_said_as_ssh_reads_it() {
        let server = Server::of(&Address::parse("sftp://me@[fe80::1]:2222/home").unwrap()).unwrap();
        assert_eq!(server.ssh, "me@fe80::1");
        assert_eq!(server.sftp, "me@[fe80::1]");
        let ssh = Ssh::default();
        let master = Master {
            socket: PathBuf::from("/run/user/1000/a dir/50%/keys-1-0"),
            program: PathBuf::from("ssh"),
            ssh: &ssh,
            server: &server,
        };
        assert_eq!(
            master.path(),
            "ControlPath=\"/run/user/1000/a dir/50%%/keys-1-0\""
        );
        drop(master);
        assert!(Server::of(&Address::parse("smb://nas/share").unwrap()).is_err());
    }
}
