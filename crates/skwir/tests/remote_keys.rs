//! Key login for SFTP set up from Skwir (`remote::keys`), against servers on
//! this machine: OpenSSH's own server run as the user, its home and
//! authorized keys the test's; OpenSSH checking a password through PAM, as
//! root of user and mount namespaces of its own, its accounts the test's;
//! and rclone's SFTP server, which checks a password and runs no command.
//! The keys listed from an agent of the test's, the default ones found in
//! a folder of the test's, a new one made with its passphrase given
//! through the `skwir` executable as `SSH_ASKPASS`.
//!
//! Skipped, saying why, where OpenSSH or rclone is missing, or the
//! namespaces cannot be made. Never the
//! user's `~/.ssh`, agent, known hosts or ssh configuration: ssh is given
//! `-F /dev/null`, known hosts and keys of the test's, each command its
//! agent by name.
#![cfg(target_os = "linux")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use skwir_core::cancel::CancelToken;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::Ask;
use skwir_plugin_remote::keys::{self, Key, Ssh};

/// A folder of the test's on the home's file system, never `/tmp`.
fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests/keys")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `None`, saying why, where one of `programs` is missing.
fn installed(programs: &[&str]) -> Option<()> {
    for program in programs {
        if skwir_core::os_path::find_program(program).is_none() {
            eprintln!("skipped: {program} is not installed");
            return None;
        }
    }
    Some(())
}

/// A key of the test's at `path`, no passphrase: `-N ""` tells no secret.
fn keygen(path: &Path, kind: &str, comment: &str) {
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", kind, "-N", "", "-C", comment, "-f"])
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success());
}

fn public(path: &Path) -> Key {
    let text = std::fs::read_to_string(path.with_extension("pub")).unwrap();
    Key::parse(&text).unwrap()
}

/// What `ssh-keygen -l` says of the key at `public`: `SHA256:…`.
fn fingerprint_of(public: &Path) -> String {
    let out = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(public)
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned()
}

fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The ssh masters this test left running: their names hold its process
/// (`keys-<pid>-<n>`), whatever ssh made of its command line.
fn masters_left() -> Vec<u32> {
    let mark = format!("skwir/keys-{}-", std::process::id());
    let mut left = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let line = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&line).contains(&mark) {
            left.push(pid);
        }
    }
    left
}

/// Until no master of this test runs, for at most five seconds.
fn no_master_left() {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = masters_left();
        if left.is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "masters left running: {left:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// An ssh agent of the test's, on a socket of its own; stopped when
/// dropped.
struct Agent {
    child: Child,
    socket: PathBuf,
}

impl Agent {
    fn start(base: &Path) -> Agent {
        let socket = base.join("agent.sock");
        let child = Command::new("ssh-agent")
            .arg("-D")
            .arg("-a")
            .arg(&socket)
            .stdout(Stdio::null())
            .spawn()
            .expect("start ssh-agent");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !socket.exists() {
            assert!(Instant::now() < deadline, "the agent never listened");
            std::thread::sleep(Duration::from_millis(20));
        }
        Agent { child, socket }
    }

    /// Give it the key at `path`, which asks nothing.
    fn add(&self, path: &Path) {
        let added = Command::new("ssh-add")
            .arg("-q")
            .arg(path)
            .env("SSH_AUTH_SOCK", &self.socket)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(added.success());
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// OpenSSH's own server, run as this user on a port of its own: its host
/// key and its user's home the test's (a command it runs starts there,
/// `HOME` said so), the keys it lets in those it was started with and
/// those of that home's `.ssh/authorized_keys`, which is not there at
/// first. Stopped when dropped.
struct Sshd {
    child: Child,
    port: u16,
    home: PathBuf,
}

impl Sshd {
    /// `None`, saying why, where OpenSSH's server is missing.
    fn start(base: &Path, authorized: &[&Key]) -> Option<Sshd> {
        let Some(sshd) = skwir_core::os_path::find_program("sshd") else {
            eprintln!("skipped: OpenSSH's server is not installed");
            return None;
        };
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let lines: String = authorized
            .iter()
            .flat_map(|key| [key.line.as_str(), "\n"])
            .collect();
        std::fs::write(base.join("login_keys"), lines).unwrap();
        keygen(&base.join("hostkey"), "ed25519", "host");
        // What the client asks runs in the test's home, never the user's,
        // with the tests' programs.
        let wrapper = base.join("in-home.sh");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nHOME='{}'; export HOME\nPATH='{}'; export PATH\n\
                 cd \"$HOME\" || exit 1\nexec sh -c \"$SSH_ORIGINAL_COMMAND\"\n",
                home.display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let port = free_port();
        let config = base.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\n\
                 AuthorizedKeysFile {} {}\nUsePAM no\nStrictModes no\n\
                 PasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 ForceCommand {}\nLogLevel ERROR\n",
                base.join("hostkey").display(),
                base.join("sshd.pid").display(),
                base.join("login_keys").display(),
                home.join(".ssh/authorized_keys").display(),
                wrapper.display(),
            ),
        )
        .unwrap();
        let child = Command::new(sshd)
            .arg("-D")
            .arg("-f")
            .arg(&config)
            .arg("-E")
            .arg(base.join("sshd.log"))
            .spawn()
            .expect("start sshd");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "sshd never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(Sshd { child, port, home })
    }
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// This user's first block of subordinate ids in `file` (`/etc/subuid`),
/// as `unshare` maps them after root: `1:<start>:<count - 1>`.
fn subordinate(file: &str) -> Option<String> {
    let user = std::env::var("USER").ok()?;
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|line| {
        let mut parts = line.split(':');
        let (name, start, count) = (parts.next()?, parts.next()?, parts.next()?);
        let count: u64 = count.trim().parse().ok()?;
        (name == user && count > 1).then(|| format!("1:{start}:{}", count - 1))
    })
}

/// The `pam_unix` module beside the PAM that OpenSSH's `sshd-session` at
/// `sshd`'s side links, as `ldd` says.
fn pam_unix_of(sshd: &Path) -> Option<PathBuf> {
    let bin = std::fs::canonicalize(sshd).ok()?;
    let session = bin.parent()?.parent()?.join("libexec").join("sshd-session");
    let linked = Command::new("ldd").arg(&session).output().ok()?;
    String::from_utf8_lossy(&linked.stdout)
        .lines()
        .find_map(|line| {
            let (_, path) = line.split_once("libpam.so.0 => ")?;
            let path = Path::new(path.split_whitespace().next()?);
            Some(path.parent()?.join("security").join("pam_unix.so"))
        })
        .filter(|module| module.is_file())
}

/// OpenSSH's own server checking a password as most do, through PAM: run
/// as root of a user namespace of its own (this user, mapped), so it reads
/// a shadow file and switches to its unprivileged half, a subordinate id
/// (newuidmap). In a mount namespace, the accounts, shadow file and PAM
/// stack of the test's own are laid over the system's (and nscd, which
/// would answer for the system's, hidden): its root logs in with
/// [`PASSWORD`] alone at first, its home the test's. Every process ends
/// with it (a pid namespace). `None`, saying why, where such namespaces
/// cannot be made, or OpenSSH's PAM, a password's hash or `unshare` is
/// missing.
struct PasswordSshd {
    child: Child,
    port: u16,
    home: PathBuf,
}

impl PasswordSshd {
    fn start(base: &Path) -> Option<PasswordSshd> {
        let found = |program: &str| {
            let found = skwir_core::os_path::find_program(program);
            if found.is_none() {
                eprintln!("skipped: {program} is not installed");
            }
            found
        };
        let (sshd, unshare, mount) = (found("sshd")?, found("unshare")?, found("mount")?);
        let (Some(users), Some(groups)) = (subordinate("/etc/subuid"), subordinate("/etc/subgid"))
        else {
            eprintln!("skipped: this user has no subordinate ids (/etc/subuid)");
            return None;
        };
        let Some(pam_unix) = pam_unix_of(&sshd) else {
            eprintln!("skipped: no pam_unix beside OpenSSH's PAM");
            return None;
        };
        // Its hash, the password on no command line.
        let mut hashing = Command::new(found("openssl")?)
            .args(["passwd", "-6", "-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut hashing.stdin.take().unwrap(), PASSWORD.as_bytes()).unwrap();
        let hashed = hashing.wait_with_output().unwrap();
        let hash = String::from_utf8(hashed.stdout).unwrap().trim().to_owned();
        assert!(hash.starts_with("$6$"), "{hash}");

        let home = base.join("home");
        for folder in ["home", "empty", "nscd", "pam.d"] {
            std::fs::create_dir_all(base.join(folder)).unwrap();
        }
        std::fs::write(
            base.join("passwd"),
            format!(
                "root:x:0:0:root:{}:/bin/sh\nsshd:x:1:1:sshd:/var/empty:/bin/false\n",
                home.display()
            ),
        )
        .unwrap();
        std::fs::write(base.join("group"), "root:x:0:\nsshd:x:1:\n").unwrap();
        std::fs::write(
            base.join("shadow"),
            format!("root:{hash}:20000:0:99999:7:::\nsshd:!:20000::::::\n"),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(base.join("shadow"), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        let mut stack = String::new();
        for kind in ["auth", "account", "password", "session"] {
            let _ = writeln!(stack, "{kind} required {}", pam_unix.display());
        }
        std::fs::write(base.join("pam.d/sshd"), stack).unwrap();
        keygen(&base.join("hostkey"), "ed25519", "host");
        // What the client asks runs in the test's home with the tests'
        // programs: sshd's own path has none of them here.
        let wrapper = base.join("in-home.sh");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nPATH='{}'; export PATH\ncd \"$HOME\" || exit 1\n\
                 exec sh -c \"$SSH_ORIGINAL_COMMAND\"\n",
                std::env::var("PATH").unwrap(),
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let port = free_port();
        let config = base.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\n\
                 UsePAM yes\nPasswordAuthentication yes\nKbdInteractiveAuthentication no\n\
                 PermitRootLogin yes\nStrictModes no\nAuthorizedKeysFile .ssh/authorized_keys\n\
                 ForceCommand {}\nLogLevel VERBOSE\n",
                base.join("hostkey").display(),
                base.join("sshd.pid").display(),
                wrapper.display(),
            ),
        )
        .unwrap();
        let inside = base.join("inside.sh");
        let laid = |from: &str, over: &str| {
            format!(
                "'{}' --bind '{}' {over}\n",
                mount.display(),
                base.join(from).display()
            )
        };
        std::fs::write(
            &inside,
            format!(
                "set -e\n{}{}{}{}{}if [ -d /run/nscd ]; then {}fi\nexec '{}' -D -f '{}' -E '{}'\n",
                laid("passwd", "/etc/passwd"),
                laid("group", "/etc/group"),
                laid("shadow", "/etc/shadow"),
                laid("pam.d", "/etc/pam.d"),
                laid("empty", "/var/empty"),
                laid("nscd", "/run/nscd"),
                sshd.display(),
                config.display(),
                base.join("sshd.log").display(),
            ),
        )
        .unwrap();
        let mut child = Command::new(unshare)
            .args(["-U", "--map-user=0", "--map-group=0"])
            .arg(format!("--map-users={users}"))
            .arg(format!("--map-groups={groups}"))
            .args([
                "-m",
                "-p",
                "--fork",
                "--kill-child",
                "--propagation",
                "private",
                "sh",
            ])
            .arg(&inside)
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(base.join("unshare.log")).unwrap())
            .spawn()
            .expect("start unshare");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            if child.try_wait().unwrap().is_some() {
                eprintln!(
                    "skipped: no user namespace with subordinate ids to run sshd as root in: {}",
                    std::fs::read_to_string(base.join("unshare.log")).unwrap_or_default()
                );
                return None;
            }
            assert!(Instant::now() < deadline, "sshd never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(PasswordSshd { child, port, home })
    }
}

impl Drop for PasswordSshd {
    fn drop(&mut self) {
        // Its namespace's first process killed with it, then all of them.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// rclone's SFTP server of `root`, user `tester` let in by password or by
/// the keys of a file, on a port of its own; its host keys kept in the
/// test's cache, so it is the same server started again. Stopped when
/// dropped.
struct Rclone {
    child: Child,
    port: u16,
    base: PathBuf,
    root: PathBuf,
}

const PASSWORD: &str = "a test password";

impl Rclone {
    fn start(base: &Path, root: &Path, authorized: &Path) -> Rclone {
        let port = free_port();
        let child = Rclone::spawn(base, root, authorized, port);
        Rclone {
            child,
            port,
            base: base.to_owned(),
            root: root.to_owned(),
        }
    }

    fn spawn(base: &Path, root: &Path, authorized: &Path, port: u16) -> Child {
        let child = Command::new("rclone")
            .args(["serve", "sftp"])
            .arg(root)
            .args(["--addr", &format!("127.0.0.1:{port}")])
            .args(["--user", "tester", "--pass", PASSWORD])
            .arg("--authorized-keys")
            .arg(authorized)
            .arg("--config")
            .arg(base.join("rclone.conf"))
            .env("XDG_CACHE_HOME", base.join("cache"))
            .env("XDG_CONFIG_HOME", base.join("config"))
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(base.join("rclone.log")).unwrap())
            .spawn()
            .expect("start rclone serve sftp");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "rclone never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        child
    }

    /// Stopped and started again, letting in the keys of `authorized`:
    /// rclone reads them as it starts.
    fn again(&mut self, authorized: &Path) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = Rclone::spawn(&self.base, &self.root, authorized, self.port);
    }
}

impl Drop for Rclone {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Answers as the window would, and says what it was asked.
#[derive(Default)]
struct Answers {
    password: String,
    asked: Mutex<Vec<String>>,
}

impl Answers {
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    fn forget(&self) {
        self.asked.lock().unwrap().clear();
    }
}

impl Ask for Answers {
    fn secret(&self, title: &str, _: &str, _: &CancelToken) -> Option<String> {
        self.asked.lock().unwrap().push(format!("secret: {title}"));
        Some(self.password.clone())
    }
    fn password(
        &self,
        title: &str,
        _: &str,
        _: &CancelToken,
    ) -> Option<skwir_plugin_remote::connector::Password> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("password: {title}"));
        // None to give: Cancel.
        (!self.password.is_empty()).then(|| skwir_plugin_remote::connector::Password {
            text: self.password.clone(),
            remember: false,
        })
    }
    fn confirm(&self, title: &str, detail: &str, _: &CancelToken) -> bool {
        self.asked
            .lock()
            .unwrap()
            .push(format!("confirm: {title} {detail}"));
        true
    }
}

/// ssh with the test's files alone: no configuration, the key at
/// `identity` (a private half, or a public one the agent holds), the agent
/// at `agent` or none, the known hosts of `known`; asking through the
/// `skwir` executable.
fn ssh_with(identity: &Path, agent: Option<&Path>, known: &Path) -> Ssh {
    Ssh {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        options: vec![
            format!("IdentityFile={}", identity.display()),
            "IdentitiesOnly=yes".to_owned(),
            format!(
                "IdentityAgent={}",
                agent.map_or_else(|| "none".to_owned(), |a| a.display().to_string())
            ),
            format!("UserKnownHostsFile={}", known.display()),
            "GlobalKnownHostsFile=/dev/null".to_owned(),
        ],
        config: Some(PathBuf::from("/dev/null")),
        ..Ssh::default()
    }
}

fn ask_of(answers: &Arc<Answers>) -> Arc<dyn Ask> {
    answers.clone()
}

#[test]
fn an_agents_keys_are_listed_and_none_where_no_agent_answers() {
    if installed(&["ssh-agent", "ssh-add", "ssh-keygen"]).is_none() {
        return;
    }
    let base = fixture("agent");
    let cancel = CancelToken::new();
    let agent = Agent::start(&base);
    assert_eq!(keys::agent_keys(Some(&agent.socket), &cancel), Vec::new());
    keygen(&base.join("a"), "ed25519", "key a");
    keygen(&base.join("b"), "ecdsa", "key b");
    agent.add(&base.join("a"));
    agent.add(&base.join("b"));
    let listed = keys::agent_keys(Some(&agent.socket), &cancel);
    assert_eq!(listed.len(), 2, "{listed:?}");
    for (key, name) in listed.iter().zip(["a", "b"]) {
        let file = base.join(name);
        assert_eq!(key.line, public(&file).line);
        assert_eq!(key.comment, format!("key {name}"));
        assert_eq!(key.fingerprint, fingerprint_of(&file.with_extension("pub")));
        assert_eq!(key.file, None, "the agent's");
    }
    assert_eq!(listed[0].kind, "ssh-ed25519");
    assert_eq!(listed[1].kind, "ecdsa-sha2-nistp256");
    // No agent there: none, and no error.
    assert_eq!(
        keys::agent_keys(Some(&base.join("no-agent.sock")), &cancel),
        Vec::new()
    );
    drop(agent);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn the_default_keys_are_found_in_ssh_s_order() {
    if installed(&["ssh-keygen"]).is_none() {
        return;
    }
    let base = fixture("default");
    let cancel = CancelToken::new();
    let dir = base.join(".ssh");
    std::fs::create_dir_all(&dir).unwrap();
    keygen(&dir.join("id_ed25519"), "ed25519", "ed");
    keygen(&dir.join("id_ecdsa"), "ecdsa", "ec");
    // Not one ssh tries by default.
    keygen(&dir.join("work"), "ed25519", "work");
    let found = keys::default_keys(&dir, &cancel);
    let names: Vec<_> = found.iter().map(|k| k.file.clone().unwrap()).collect();
    assert_eq!(names, [dir.join("id_ecdsa"), dir.join("id_ed25519")]);
    assert_eq!(found[0].comment, "ec");
    assert_eq!(found[1].line, public(&dir.join("id_ed25519")).line);
    assert_eq!(
        found[1].fingerprint,
        fingerprint_of(&dir.join("id_ed25519.pub"))
    );
    assert_eq!(keys::default_keys(&base.join("none"), &cancel), Vec::new());
    std::fs::remove_dir_all(&base).unwrap();
}

/// A new key's passphrase.
const PASSPHRASE: &str = "a long passphrase of the test's";

#[test]
fn a_new_key_is_encrypted_by_its_passphrase_never_on_a_command_line() {
    if installed(&["ssh-keygen"]).is_none() {
        return;
    }
    let base = fixture("new");
    let cancel = CancelToken::new();
    // The askpass ssh-keygen runs notes the command line of what ran it,
    // then asks through `skwir`.
    let noted = base.join("command-lines");
    let askpass = base.join("askpass.sh");
    std::fs::write(
        &askpass,
        format!(
            "#!/bin/sh\ntr '\\0' ' ' < /proc/$PPID/cmdline >> '{}'\necho >> '{}'\nexec '{}' \"$@\"\n",
            noted.display(),
            noted.display(),
            env!("CARGO_BIN_EXE_skwir"),
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ssh = Ssh {
        askpass: Some(askpass),
        ..Ssh::default()
    };
    let dir = base.join("home/.ssh");
    let key = keys::new_key(&dir, "test@skwir", PASSPHRASE, &ssh, &cancel).expect("new key");
    let file = dir.join("id_ed25519");
    assert_eq!(key.file.as_deref(), Some(file.as_path()));
    assert_eq!(key.kind, "ssh-ed25519");
    assert_eq!(key.comment, "test@skwir");
    assert_eq!(key.line, public(&file).line);
    assert_eq!(key.fingerprint, fingerprint_of(&file.with_extension("pub")));
    assert_eq!(mode(&dir), 0o700, "the folder, made private");
    assert_eq!(mode(&file), 0o600);
    // Asked twice, through askpass; its command line never held it.
    let lines = std::fs::read_to_string(&noted).unwrap();
    let lines: Vec<&str> = lines.lines().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    for line in &lines {
        assert!(line.contains("ssh-keygen"), "{line}");
        assert!(!line.contains(PASSPHRASE), "{line}");
    }
    // Encrypted: a wrong passphrase opens nothing, the right one, given as
    // the window gives it, opens it.
    let wrong = Command::new("ssh-keygen")
        .args(["-y", "-P", "wrong", "-f"])
        .arg(&file)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    let folder = base.join("ask");
    std::fs::create_dir_all(&folder).unwrap();
    let server =
        skwir_platform::askpass::Server::start(&folder, |_| Some(PASSPHRASE.to_owned())).unwrap();
    let right = Command::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(&file)
        .env("SSH_ASKPASS", env!("CARGO_BIN_EXE_skwir"))
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env(skwir_platform::askpass::SOCKET_VAR, server.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    drop(server);
    assert!(right.status.success(), "{right:?}");
    let opened = Key::parse(&String::from_utf8(right.stdout).unwrap()).unwrap();
    assert!(opened.same(&key));

    // A key there is never replaced.
    let before = std::fs::read(&file).unwrap();
    assert!(keys::new_key(&dir, "again", "", &ssh, &cancel).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before);

    // No passphrase: nothing asked, a key that opens with none.
    let plain = base.join("plain/.ssh");
    let key = keys::new_key(&plain, "plain", "", &ssh, &cancel).expect("a key with none");
    assert_eq!(std::fs::read_to_string(&noted).unwrap().lines().count(), 2);
    let open = Command::new("ssh-keygen")
        .args(["-y", "-P", "wrong", "-f"])
        .arg(key.file.unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(open.status.success(), "it needs none: {open:?}");
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn key_login_is_set_up_through_the_server_s_shell_its_questions_asked_of_the_window() {
    if installed(&["ssh", "ssh-keygen", "ssh-agent", "ssh-add"]).is_none() {
        return;
    }
    let base = fixture("shell");
    let cancel = CancelToken::new();
    for name in ["a", "b", "c"] {
        keygen(&base.join(name), "ed25519", &format!("key {name}"));
    }
    let (a, b) = (public(&base.join("a")), public(&base.join("b")));
    // The server lets in key a alone.
    let Some(sshd) = Sshd::start(&base, &[&a]) else {
        return;
    };
    let address = Address::parse(&format!("sftp://127.0.0.1:{}/", sshd.port)).unwrap();
    let known = base.join("known_hosts");
    let by_a = ssh_with(&base.join("a"), None, &known);
    let by_b = ssh_with(&base.join("b"), None, &known);
    let answers = Arc::new(Answers::default());
    let ask = ask_of(&answers);

    // The host never seen is asked about through the `skwir` executable.
    assert!(keys::logs_in(&address, &by_a, Some(&ask), &cancel));
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("confirm:") && one.contains("127.0.0.1")),
        "{asked:?}"
    );
    // Key b, the host known by now, is refused.
    assert!(!keys::logs_in(&address, &by_b, None, &cancel));

    // Put up while key a logs in: nothing asked, the folder and the file
    // made with only the user allowed in.
    answers.forget();
    let folder = sshd.home.join(".ssh");
    let authorized = folder.join("authorized_keys");
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_a, &ask, &cancel).unwrap(),
        1
    );
    assert_eq!(answers.asked(), Vec::<String>::new());
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );
    assert_eq!((mode(&folder), mode(&authorized)), (0o700, 0o600));
    assert!(keys::logs_in(&address, &by_b, None, &cancel));

    // Those it has are not added again, whoever logs in.
    assert_eq!(
        keys::install(&address, &[b.clone(), b.clone()], &by_b, &ask, &cancel).unwrap(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );
    // Its own question about the host reaches the window too.
    let by_a_afresh = ssh_with(&base.join("a"), None, &base.join("known_hosts-2"));
    assert_eq!(
        keys::install(
            &address,
            std::slice::from_ref(&b),
            &by_a_afresh,
            &ask,
            &cancel
        )
        .unwrap(),
        0
    );
    assert!(
        answers.asked().iter().any(|q| q.starts_with("confirm:")),
        "{:?}",
        answers.asked()
    );

    // A key the agent alone holds: listed, put up (added in place by the
    // server's shell: sftp would put a new file there), and logging in
    // from there, its file gone. A file ending without a new line is
    // ended first.
    let agent = Agent::start(&base);
    agent.add(&base.join("c"));
    std::fs::remove_file(base.join("c")).unwrap();
    let held = keys::agent_keys(Some(&agent.socket), &cancel);
    assert_eq!(held.len(), 1);
    let by_c = ssh_with(&base.join("c.pub"), Some(&agent.socket), &known);
    assert!(!keys::logs_in(&address, &by_c, None, &cancel));
    std::fs::write(&authorized, &b.line).unwrap();
    let inode =
        |path: &Path| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(path).unwrap());
    let before = inode(&authorized);
    assert_eq!(
        keys::install(&address, &held, &by_a, &ask, &cancel).unwrap(),
        1
    );
    assert_eq!(inode(&authorized), before);
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n{}\n", b.line, held[0].line)
    );
    assert!(keys::logs_in(&address, &by_c, None, &cancel));

    // A new key with a passphrase, put up: it logs in once its passphrase
    // is asked of the window, as a key's (never kept), and not without.
    let made = Ssh {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        ..Ssh::default()
    };
    let new = keys::new_key(&base.join("mine/.ssh"), "new", PASSPHRASE, &made, &cancel).unwrap();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&new), &by_a, &ask, &cancel).unwrap(),
        1
    );
    let by_new = ssh_with(new.file.as_deref().unwrap(), None, &known);
    assert!(!keys::logs_in(&address, &by_new, None, &cancel));
    let typing = Arc::new(Answers {
        password: PASSPHRASE.to_owned(),
        ..Answers::default()
    });
    assert!(keys::logs_in(
        &address,
        &by_new,
        Some(&ask_of(&typing)),
        &cancel
    ));
    let asked = typing.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("secret:") && one.contains("passphrase")),
        "{asked:?}"
    );
    no_master_left();
    drop(agent);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn key_login_is_set_up_by_the_server_s_password_on_a_server_of_sftp_alone() {
    if installed(&["ssh", "sftp", "ssh-keygen", "rclone"]).is_none() {
        return;
    }
    let base = fixture("sftp");
    let cancel = CancelToken::new();
    keygen(&base.join("b"), "ed25519", "key b");
    let b = public(&base.join("b"));
    // At first no key gets in: the password alone.
    let none = base.join("no-keys");
    std::fs::write(&none, "").unwrap();
    let root = base.join("served");
    std::fs::create_dir_all(&root).unwrap();
    let mut rclone = Rclone::start(&base, &root, &none);
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", rclone.port)).unwrap();
    let by_b = ssh_with(&base.join("b"), None, &base.join("known_hosts"));
    let answers = Arc::new(Answers {
        password: PASSWORD.to_owned(),
        ..Answers::default()
    });
    let ask = ask_of(&answers);

    // Not by key, and no password asked to find out.
    assert!(!keys::logs_in(&address, &by_b, Some(&ask), &cancel));
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("confirm:")),
        "the host alone: {asked:?}"
    );

    // Its password refused by the user, or a wrong one: nothing put up,
    // and said so.
    let refusing = Arc::new(Answers::default());
    let error = keys::install(
        &address,
        std::slice::from_ref(&b),
        &by_b,
        &ask_of(&refusing),
        &cancel,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "you cancelled");
    let wrong = Arc::new(Answers {
        password: "not the password".to_owned(),
        ..Answers::default()
    });
    let error = keys::install(
        &address,
        std::slice::from_ref(&b),
        &by_b,
        &ask_of(&wrong),
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refused the login"), "{error}");
    assert!(!root.join(".ssh").exists());

    // Its password asked once, as a server's; the key put up through sftp
    // (rclone runs no shell), its folder made.
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        1
    );
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("password:") && one.contains("tester@127.0.0.1")),
        "{asked:?}"
    );
    let authorized = root.join(".ssh/authorized_keys");
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );
    let left: Vec<_> = std::fs::read_dir(root.join(".ssh"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["authorized_keys"], "nothing staged left there");

    // Started again with those keys, it lets key b in.
    rclone.again(&authorized);
    assert!(keys::logs_in(&address, &by_b, None, &cancel));
    // Again: in by key, nothing asked, nothing added.
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        0
    );
    assert_eq!(answers.asked(), Vec::<String>::new());
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );
    no_master_left();
    drop(rclone);
    std::fs::remove_dir_all(&base).unwrap();
}

/// As on Windows, whose ssh makes no master: each ssh and sftp logs in
/// alone. On a server of SFTP alone (rclone's runs no command) the keys go
/// up through sftp all the same, the server's password asked once and
/// given to the sessions after; how ssh ended says whether it logged in.
#[test]
fn key_login_is_set_up_on_a_server_of_sftp_alone_where_no_login_is_shared() {
    if installed(&["ssh", "sftp", "ssh-keygen", "rclone"]).is_none() {
        return;
    }
    let base = fixture("alone");
    let cancel = CancelToken::new();
    keygen(&base.join("b"), "ed25519", "key b");
    let b = public(&base.join("b"));
    let none = base.join("no-keys");
    std::fs::write(&none, "").unwrap();
    let root = base.join("served");
    std::fs::create_dir_all(&root).unwrap();
    let mut rclone = Rclone::start(&base, &root, &none);
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", rclone.port)).unwrap();
    let by_b = Ssh {
        alone: true,
        ..ssh_with(&base.join("b"), None, &base.join("known_hosts"))
    };
    let answers = Arc::new(Answers {
        password: PASSWORD.to_owned(),
        ..Answers::default()
    });
    let ask = ask_of(&answers);

    // Not by key, the host alone asked.
    assert!(!keys::logs_in(&address, &by_b, Some(&ask), &cancel));
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("confirm:")),
        "the host alone: {asked:?}"
    );

    // A wrong password: nothing put up, and said so.
    let wrong = Arc::new(Answers {
        password: "not the password".to_owned(),
        ..Answers::default()
    });
    let error = keys::install(
        &address,
        std::slice::from_ref(&b),
        &by_b,
        &ask_of(&wrong),
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refused the login"), "{error}");
    assert!(!root.join(".ssh").exists());

    // Its password asked once, for ssh's login, and given to the two sftp
    // sessions after: the key put up, its folder made.
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        1
    );
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("password:") && one.contains("tester@127.0.0.1")),
        "{asked:?}"
    );
    let authorized = root.join(".ssh/authorized_keys");
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );

    // Started again with those keys: in by key, though no command runs
    // there; nothing asked, nothing added again.
    rclone.again(&authorized);
    assert!(keys::logs_in(&address, &by_b, None, &cancel));
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        0
    );
    assert_eq!(answers.asked(), Vec::<String>::new());
    no_master_left();
    drop(rclone);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Against OpenSSH checking a password, as a server most often is: the
/// password asked of the window as a server's (`root@127.0.0.1's
/// password:`), the key put up through its shell, and a login by key
/// after. A wrong one and one refused put up nothing.
#[test]
fn key_login_is_set_up_by_an_openssh_server_s_password() {
    if installed(&["ssh", "ssh-keygen"]).is_none() {
        return;
    }
    let base = fixture("password");
    let cancel = CancelToken::new();
    keygen(&base.join("b"), "ed25519", "key b");
    let b = public(&base.join("b"));
    let Some(sshd) = PasswordSshd::start(&base) else {
        return;
    };
    let address = Address::parse(&format!("sftp://root@127.0.0.1:{}/", sshd.port)).unwrap();
    let by_b = ssh_with(&base.join("b"), None, &base.join("known_hosts"));
    let answers = Arc::new(Answers {
        password: PASSWORD.to_owned(),
        ..Answers::default()
    });
    let ask = ask_of(&answers);

    // Not by key, and no password asked to find out.
    assert!(!keys::logs_in(&address, &by_b, Some(&ask), &cancel));
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("confirm:")),
        "the host alone: {asked:?}"
    );

    // Refused by the user, or a wrong one (asked as often as ssh asks):
    // nothing put up, and said so.
    let refusing = Arc::new(Answers::default());
    let error = keys::install(
        &address,
        std::slice::from_ref(&b),
        &by_b,
        &ask_of(&refusing),
        &cancel,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "you cancelled");
    let wrong = Arc::new(Answers {
        password: "not the password".to_owned(),
        ..Answers::default()
    });
    let error = keys::install(
        &address,
        std::slice::from_ref(&b),
        &by_b,
        &ask_of(&wrong),
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refused the login"), "{error}");
    let asked = wrong.asked();
    assert!(
        !asked.is_empty() && asked.iter().all(|q| q.starts_with("password:")),
        "{asked:?}"
    );
    let folder = sshd.home.join(".ssh");
    assert!(!folder.exists());

    // Its password asked once, as a server's; the key put up through its
    // shell, the folder and the file made with only the user allowed in.
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        1
    );
    let asked = answers.asked();
    assert!(
        matches!(asked.as_slice(), [one] if one.starts_with("password:") && one.contains("root@127.0.0.1")),
        "{asked:?}"
    );
    let authorized = folder.join("authorized_keys");
    assert_eq!(
        std::fs::read_to_string(&authorized).unwrap(),
        format!("{}\n", b.line)
    );
    assert_eq!((mode(&folder), mode(&authorized)), (0o700, 0o600));

    // In by key from now on: nothing asked, nothing added again.
    assert!(keys::logs_in(&address, &by_b, None, &cancel));
    answers.forget();
    assert_eq!(
        keys::install(&address, std::slice::from_ref(&b), &by_b, &ask, &cancel).unwrap(),
        0
    );
    assert_eq!(answers.asked(), Vec::<String>::new());
    no_master_left();
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Set Up Key Login from a server's menu, in its form: the agent's key
/// offered first, then the default ones and a new one; the one chosen
/// put up on the server (its own `.ssh`, never the user's), the window
/// told.
#[test]
fn key_login_is_set_up_from_a_server_s_form() {
    use skwir_core::command::Invocation;
    use skwir_core::form::{Answer, Kind, Values};
    if installed(&["ssh", "ssh-keygen", "ssh-agent", "ssh-add"]).is_none() {
        return;
    }
    let base = fixture("form");
    for name in ["a", "b"] {
        keygen(&base.join(name), "ed25519", &format!("key {name}"));
    }
    let (a, b) = (public(&base.join("a")), public(&base.join("b")));
    // The server lets in key a alone; the agent holds key b.
    let Some(sshd) = Sshd::start(&base, &[&a]) else {
        return;
    };
    let agent = Agent::start(&base);
    agent.add(&base.join("b"));
    let mut ssh = ssh_with(
        &base.join("a"),
        Some(&agent.socket),
        &base.join("known_hosts"),
    );
    ssh.options
        .push("StrictHostKeyChecking=accept-new".to_owned());
    let app =
        skwir_core::app::App::new(skwir_core::app::AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes =
        skwir_plugin_remote::Remotes::with(Vec::new()).with_keys(skwir_plugin_remote::KeySetup {
            ssh,
            ssh_dir: Some(base.join("dot-ssh")),
            agent: Some(agent.socket.clone()),
        });
    app.plugins.add(
        Arc::new(skwir_plugin_remote::RemotePlugin::with(remotes)),
        &app,
    );
    app.set_window_shown();
    let address = format!("sftp://127.0.0.1:{}/", sshd.port);
    app.dispatch(&Invocation::with_args(
        "remote.setup_key_login",
        &serde_json::json!({ "address": address }),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let asked = loop {
        app.pump();
        if let Some(asked) = app.take_forms().pop() {
            break asked;
        }
        assert!(Instant::now() < deadline, "no form was asked");
        std::thread::sleep(Duration::from_millis(20));
    };
    let Kind::Choice { choices, .. } = &asked.form.fields[0].kind else {
        panic!("a choice of keys");
    };
    assert_eq!(choices[0].value, "agent:0", "{choices:?}");
    assert!(choices[0].title.contains("key b"), "{}", choices[0].title);
    assert_eq!(choices.last().unwrap().value, "new");
    let mut values = Values::default();
    values.set("key", "agent:0");
    values.set("passphrase", "");
    values.set("again", "");
    assert!(app.answer_form(asked.id, Some(Answer::Accepted(values))));
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut heard = Vec::new();
    loop {
        app.pump();
        heard.extend(app.take_notifications().into_iter().map(|n| n.text));
        if heard.iter().any(|t| t.contains("Key login is set up")) {
            break;
        }
        assert!(Instant::now() < deadline, "never set up: {heard:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let authorized = std::fs::read_to_string(sshd.home.join(".ssh/authorized_keys")).unwrap();
    assert_eq!(
        authorized,
        format!("{}\n", b.line),
        "the agent's key, put up"
    );
    assert!(!base.join("dot-ssh").exists(), "no key made");
    drop(agent);
    drop(sshd);
    no_master_left();
    std::fs::remove_dir_all(&base).unwrap();
}
