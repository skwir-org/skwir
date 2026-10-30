//! Remote folders as the window has them (ADR-077), against servers on
//! this machine, mounted for real through FUSE: an address navigated to
//! connects and shows the mounted folder; what is known of the remote
//! (its address, its thumbnails) is published; a tab keeps its address;
//! Move to Trash deletes for good there; Eject lets go of it. And sshfs
//! through ssh, its password and an unknown host asked of the window
//! through the `skwir` executable as `SSH_ASKPASS`.
//!
//! Skipped, saying why, where rclone, sshfs or FUSE is missing. Never the
//! user's GVfs, configuration or known hosts: each test names its
//! connector and gives ssh files of its own.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::{Ask, Connector, Password};
use skwir_plugin_remote::kio::Kio;
use skwir_plugin_remote::passwords::{Keyed, Keyring, Noted};
use skwir_plugin_remote::rclone::Rclone;
use skwir_plugin_remote::sshfs::Sshfs;
use skwir_plugin_remote::{RemotePlugin, Remotes};
use std::sync::Mutex;

/// A folder of the test's on the home's file system, never `/tmp`.
fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests/remote")
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

/// `rclone serve <kind>` of `root` on a port of its own, stopped when
/// dropped.
struct Served {
    child: Child,
    port: u16,
}

impl Served {
    fn start(kind: &str, root: &Path, extra: &[&str]) -> Served {
        let port = free_port();
        let child = Command::new("rclone")
            .args(["serve", kind])
            .arg(root)
            .args(["--addr", &format!("127.0.0.1:{port}")])
            .args(extra)
            .arg("--config")
            .arg(root.with_extension("conf"))
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.with_extension(format!("{kind}.log"))).unwrap())
            .spawn()
            .expect("start rclone serve");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the server never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Served { child, port }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Until `ready` holds, pumping the app, for at most `seconds`.
fn until(app: &App, seconds: u64, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.pump();
        if ready() {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_address_opens_its_mounted_folder_and_the_window_keeps_it_by_its_address() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("window");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("sub folder")).unwrap();
    std::fs::write(served_root.join("sub folder/hello.txt"), "hello").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    // rclone alone: never the session's GVfs.
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_places::PlacesPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    let address = format!("dav://127.0.0.1:{}/sub folder", served.port);
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let shown = app.pane_location(pane).unwrap();
    let folder = shown.path().unwrap();
    let remote = skwir_core::fs::reach::of(&folder).expect("a remote's");
    // A WebDAV share is where its server serves it: the folder asked is
    // what is mounted.
    assert_eq!(remote.point, folder, "the folder asked, mounted");
    // Failed midway, the test lets go of it before its server stops: a
    // mount whose server is gone holds whatever looks into it.
    let _left = Unmounted(remote.point.clone());
    let names: Vec<String> = {
        let ws = app.workspace.read();
        ws.pane(pane)
            .unwrap()
            .listing
            .iter()
            .map(|e| e.name_str().into_owned())
            .collect()
    };
    assert_eq!(names, ["hello.txt"]);
    {
        let ws = app.workspace.read();
        assert_eq!(
            ws.pane(pane).unwrap().history.last(),
            Some(&shown),
            "the folder in the address's place"
        );
    }

    // What the plugin knows of it: on this machine, its thumbnails shown.
    until(&app, 20, "the remote was named", || {
        app.remote(&folder)
            .is_some_and(|(_, known)| known.is_some())
    });
    assert!(app.shows_thumbnail(&folder.join("hello.txt"), Some(5)));
    let kept = app.address_of(&shown).expect("kept by its address");
    assert_eq!(kept.to_string(), address);
    let saved = app.saved_tabs();
    assert_eq!(saved.tabs[0].panes[0], kept, "a tab keeps the address");
    let place = app
        .registry
        .places
        .find(|p| p.group == "network" && p.location == Location::from_path(&remote.point))
        .expect("a Network place");
    assert_eq!(
        place.title,
        format!("127.0.0.1:{}", served.port),
        "named by its server, its port not the usual one"
    );
    assert!(place.eject.is_some());

    // Bookmarked, it is kept by its address, in the desktop's shared
    // bookmarks (the test's own, its XDG folders being its own).
    let bookmarks = skwir_core::xdg::config_home()
        .unwrap()
        .join("gtk-3.0/bookmarks");
    let uri = format!("dav://127.0.0.1:{}/sub%20folder", served.port);
    app.dispatch(&Invocation::with_args(
        "places.bookmark",
        &serde_json::json!({ "path": folder }),
    ))
    .unwrap();
    until(&app, 10, "bookmarked by its address", || {
        std::fs::read_to_string(&bookmarks).is_ok_and(|text| text.lines().any(|l| l == uri))
    });
    app.dispatch(&Invocation::with_args(
        "places.unbookmark_address",
        &serde_json::json!({ "address": uri }),
    ))
    .unwrap();
    until(&app, 10, "the bookmark taken out", || {
        !std::fs::read_to_string(&bookmarks).is_ok_and(|text| text.lines().any(|l| l == uri))
    });

    // Move to Trash deletes for good there, asked as a deletion is.
    let hello = folder.join("hello.txt");
    let plan = match app
        .dispatch(&Invocation::with_args(
            "trash",
            &serde_json::json!({ "entries": [hello] }),
        ))
        .unwrap()
    {
        Outcome::Plan(plan) => plan,
        other => panic!("{other:?}"),
    };
    assert!(!plan.undoable(), "a deletion for good");
    assert!(plan.note.as_deref().is_some_and(|n| n.contains("no trash")));
    let done = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(done.error.is_none(), "{:?}", done.error);
    assert!(!served_root.join("sub folder/hello.txt").exists());

    // Eject: what showed it goes home, the mount and its folder go.
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": remote.point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&remote.point).is_none_or(|r| r.point != remote.point)
    });
    assert!(
        app.pane_location(pane)
            .and_then(|l| l.path())
            .is_none_or(|p| !p.starts_with(&remote.point))
    );
    until(&app, 10, "its mount point removed", || {
        !remote.point.exists()
    });
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// What lets go of a FUSE mount the test left: the system's setuid wrapper
/// where it has one (NixOS: the store's `fusermount3` may not unmount),
/// else the one on the `PATH`.
fn fusermount() -> PathBuf {
    let wrapper = Path::new("/run/wrappers/bin/fusermount3");
    if wrapper.exists() {
        wrapper.to_path_buf()
    } else {
        PathBuf::from("fusermount3")
    }
}

/// A mount point let go of when dropped, if the test left it mounted.
struct Unmounted(PathBuf);

impl Drop for Unmounted {
    fn drop(&mut self) {
        if skwir_core::fs::reach::of(&self.0).is_some_and(|r| r.point == self.0) {
            // Lazily: what the app still holds open there lets go after.
            let _ = Command::new(fusermount()).arg("-uz").arg(&self.0).status();
        }
    }
}

/// Whether this system gives each mount a number no later mount takes
/// again (`STATX_MNT_ID_UNIQUE`, Linux 6.8).
fn unique_mount_numbers() -> bool {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let mut parts = release
        .split(['.', '-'])
        .map(|n| n.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (6, 8)
}

#[test]
fn a_folder_mounted_again_holds_other_files_by_number_and_the_same_by_path() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("again");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("it's here.txt"), "here").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let address = Location::parse(&format!("dav://127.0.0.1:{}/", served.port));
    let connect = || {
        app.navigate(pane, &address).unwrap();
        until(&app, 60, "the address led to its folder", || {
            app.pane_location(pane).is_some_and(|l| l.is_path())
        });
        assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
        app.pane_location(pane).unwrap().path().unwrap()
    };
    let disconnect = |point: &Path| {
        app.dispatch(&Invocation::with_args(
            "remote.disconnect",
            &serde_json::json!({ "path": point }),
        ))
        .unwrap();
        until(&app, 30, "it was let go of", || {
            skwir_core::fs::reach::of(point).is_none_or(|r| r.point != point)
        });
    };

    let folder = connect();
    let file = folder.join("it's here.txt");
    let known = skwir_core::op::EntryRef::from_path(&file).expect("its file");
    assert!(!known.id.lasts(), "a number its mount made up");
    let listed = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        p.listing
            .iter()
            .map(skwir_core::Entry::file)
            .collect::<Vec<_>>()
    };
    assert_eq!(listed, [known.id], "listed as a stat names it");
    let point = skwir_core::fs::reach::of(&folder).unwrap().point.clone();
    let left = Unmounted(point.clone());
    disconnect(&point);

    assert_eq!(connect(), folder, "mounted again where it was");
    let again = skwir_core::fs::stat::id_of(&file).expect("its file again");
    // rclone numbers a file by its path, and the kernel gives the mount
    // the lowest free device: often the one the last mount had.
    assert!(!again.lasts());
    if unique_mount_numbers() {
        assert_ne!(again, known.id, "another mount's file");
    }
    assert_eq!(known.resolve().unwrap(), file, "found at its path");
    disconnect(&point);
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A mount let go of elsewhere while Skwir still knew it (by the user, or
/// by its program: rclone does once Eject cut its connection, maybe before
/// Skwir lets go of it) is let go of all the same, its folder removed.
#[test]
fn a_mount_let_go_of_elsewhere_is_disconnected_all_the_same() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("let-go-elsewhere");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let address = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let rclone = Rclone::default();
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let connected = rclone
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let left = Unmounted(connected.point.clone());
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = rclone.recognises(&remote).expect("rclone's");
    assert!(
        Command::new("fusermount3")
            .arg("-u")
            .arg(&connected.point)
            .status()
            .unwrap()
            .success()
    );
    rclone
        .disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    assert!(!connected.point.exists(), "its folder removed");
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Answers as the window would, and says what it was asked.
#[derive(Default)]
struct Answers {
    password: String,
    /// A server's password answered with Remember on (ADR-082).
    remember: bool,
    asked: Mutex<Vec<String>>,
}

impl Ask for Answers {
    fn secret(&self, title: &str, _: &str, _: &CancelToken) -> Option<String> {
        self.asked.lock().unwrap().push(format!("secret: {title}"));
        Some(self.password.clone())
    }
    fn password(&self, title: &str, _: &str, _: &CancelToken) -> Option<Password> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("password: {title}"));
        Some(Password {
            text: self.password.clone(),
            remember: self.remember,
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

#[test]
fn sshfs_asks_the_window_for_a_password_and_an_unknown_host_through_skwir() {
    let sshfs = Sshfs {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        ..Sshfs::default()
    };
    if let Some(why) = sshfs
        .unavailable()
        .or_else(|| Rclone::default().unavailable())
    {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("sshfs");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("over ssh.txt"), "hi").unwrap();
    let served = Served::start(
        "sftp",
        &served_root,
        &["--user", "tester", "--pass", "a test password"],
    );
    // ssh with files of the test's alone: no configuration, no agent, no
    // key of the user's, its known hosts empty, so the host is unknown.
    let sshfs = Sshfs {
        ssh_options: vec![
            format!("UserKnownHostsFile={}", base.join("known_hosts").display()),
            "IdentityAgent=none".to_owned(),
            "IdentitiesOnly=yes".to_owned(),
            "PubkeyAuthentication=no".to_owned(),
        ],
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    };
    let answers = Arc::new(Answers {
        password: "a test password".to_owned(),
        remember: true,
        ..Answers::default()
    });
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", served.port)).unwrap();
    // Its password asked through the keyring's layer (ADR-082), kept once
    // the connection served.
    let keyring = Arc::new(TestKeyring::default());
    let keyed = Arc::new(Keyed::new(
        answers.clone(),
        keyring.clone(),
        address.top(),
        CancelToken::new(),
    ));
    let ask: Arc<dyn Ask> = keyed.clone();
    let connected = sshfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let asked = answers.asked.lock().unwrap().clone();
    assert!(
        asked
            .iter()
            .any(|q| q.starts_with("confirm:") && q.contains("127.0.0.1")),
        "the unknown host was asked about: {asked:?}"
    );
    assert!(
        asked
            .iter()
            .any(|q| q.starts_with("password:") && q.contains("password")),
        "its password was asked for, as a server's: {asked:?}"
    );
    let key = format!("sftp://tester@127.0.0.1:{}", served.port);
    assert_eq!(keyed.served(), Some(Noted::Kept(key.clone())));
    assert_eq!(
        keyring.own.lock().unwrap().get(&key).map(String::as_str),
        Some("a test password")
    );
    let names: Vec<String> = std::fs::read_dir(&connected.point)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["over ssh.txt"]);
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    assert_eq!(remote.fstype, "fuse.sshfs");
    let recognised = sshfs.recognises(&remote).expect("its own");
    assert!(recognised.ours);
    assert_eq!(recognised.address, Some(address.top()));
    sshfs
        .disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    assert!(!connected.point.exists());

    // Again: the keyring answers ssh's question, the window is asked
    // nothing (the host is known by now).
    let quiet = Arc::new(Answers::default());
    let keyed = Arc::new(Keyed::new(
        quiet.clone(),
        keyring.clone(),
        address.top(),
        CancelToken::new(),
    ));
    let ask: Arc<dyn Ask> = keyed.clone();
    let connected = sshfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect with the kept password");
    assert_eq!(*quiet.asked.lock().unwrap(), Vec::<String>::new());
    assert_eq!(keyed.served(), None);
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = sshfs.recognises(&remote).expect("its own");
    sshfs
        .disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A keyring in memory (ADR-082): Skwir's own passwords by key, the
/// desktop's by their attributes (GVfs's).
#[derive(Default)]
struct TestKeyring {
    own: Mutex<std::collections::HashMap<String, String>>,
    desktop: Mutex<Vec<(Vec<(String, String)>, String)>>,
}

impl Keyring for TestKeyring {
    fn own(&self, server: &str) -> Option<String> {
        self.own.lock().unwrap().get(server).cloned()
    }
    fn desktop(&self, named: &[(&str, &str)]) -> Option<String> {
        self.desktop
            .lock()
            .unwrap()
            .iter()
            .find(|(theirs, _)| {
                named
                    .iter()
                    .all(|(k, v)| theirs.iter().any(|(tk, tv)| tk == k && tv == v))
            })
            .map(|(_, secret)| secret.clone())
    }
    fn keep(&self, server: &str, _: &str, password: &str) -> anyhow::Result<()> {
        self.own
            .lock()
            .unwrap()
            .insert(server.to_owned(), password.to_owned());
        Ok(())
    }
    fn forget(&self, server: &str) -> anyhow::Result<()> {
        self.own.lock().unwrap().remove(server);
        Ok(())
    }
}

/// The processes `pid` started, theirs, and so on: read from the system's
/// list of each one's children, never by name.
fn descendants(pid: u32) -> Vec<u32> {
    let mut pids = vec![pid];
    let mut at = 0;
    while at < pids.len() {
        let tasks = std::fs::read_dir(format!("/proc/{}/task", pids[at]));
        for task in tasks.into_iter().flatten().flatten() {
            let children = std::fs::read_to_string(task.path().join("children"));
            for child in children.unwrap_or_default().split_whitespace() {
                if let Ok(pid) = child.parse::<u32>()
                    && !pids.contains(&pid)
                {
                    pids.push(pid);
                }
            }
        }
        at += 1;
    }
    pids.remove(0);
    pids
}

/// Send `signal` to each of `pids` at once (one `kill`).
fn signal(signal: &str, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .arg(signal)
        .args(pids.iter().map(u32::to_string))
        .stderr(Stdio::null())
        .status();
}

/// OpenSSH's own server, run as this user on a port of its own, with keys
/// and known hosts of the test's: rsync on its far end (`rclone serve
/// sftp` runs no command). Stopped when dropped.
struct OwnSshd {
    child: Child,
    port: u16,
    /// What ssh is given to reach it: the test's key and known hosts.
    options: Vec<String>,
    /// How it is started again ([`OwnSshd::start_again`]).
    command: (PathBuf, PathBuf, PathBuf),
}

impl OwnSshd {
    /// `None`, saying why, where OpenSSH's server or rsync is missing.
    fn start(base: &Path) -> Option<OwnSshd> {
        OwnSshd::start_with(base, "")
    }

    /// [`OwnSshd::start`], its configuration's lines `more` after its own
    /// (`ForceCommand internal-sftp`: an SFTP-only account).
    fn start_with(base: &Path, more: &str) -> Option<OwnSshd> {
        let find = skwir_core::os_path::find_program;
        let (Some(sshd), Some(keygen), Some(_)) = (find("sshd"), find("ssh-keygen"), find("rsync"))
        else {
            eprintln!("skipped: OpenSSH's server or rsync is not installed");
            return None;
        };
        let sftp = std::fs::canonicalize(&sshd)
            .ok()?
            .parent()?
            .parent()?
            .join("libexec/sftp-server");
        if !sftp.exists() {
            eprintln!("skipped: no sftp-server beside sshd");
            return None;
        }
        for key in ["key", "hostkey"] {
            let made = Command::new(&keygen)
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f"])
                .arg(base.join(key))
                .status()
                .unwrap();
            assert!(made.success());
        }
        std::fs::copy(base.join("key.pub"), base.join("authorized_keys")).unwrap();
        let port = free_port();
        let config = base.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\n\
                 AuthorizedKeysFile {}\nUsePAM no\nStrictModes no\n\
                 PasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 Subsystem sftp {}\nLogLevel ERROR\n{more}",
                base.join("hostkey").display(),
                base.join("sshd.pid").display(),
                base.join("authorized_keys").display(),
                sftp.display(),
            ),
        )
        .unwrap();
        let command = (sshd, config, base.join("sshd.log"));
        let child = OwnSshd::spawn(&command, port);
        Some(OwnSshd {
            child,
            port,
            command,
            options: vec![
                format!("IdentityFile={}", base.join("key").display()),
                "IdentitiesOnly=yes".to_owned(),
                "IdentityAgent=none".to_owned(),
                format!("UserKnownHostsFile={}", base.join("known_hosts").display()),
                "StrictHostKeyChecking=accept-new".to_owned(),
            ],
        })
    }
}

impl OwnSshd {
    /// sshd as `command` says, once it listens on `port`.
    fn spawn((sshd, config, log): &(PathBuf, PathBuf, PathBuf), port: u16) -> Child {
        let child = Command::new(sshd)
            .arg("-D")
            .arg("-f")
            .arg(config)
            .arg("-E")
            .arg(log)
            .spawn()
            .expect("start sshd");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "sshd never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        child
    }

    /// The processes it started: its sessions and what runs in them, read
    /// from the kernel's lists of children (one read each, quick: a copy
    /// on this machine goes on meanwhile).
    fn descendants(&self) -> Vec<u32> {
        descendants(self.child.id())
    }

    /// Stop it, the sessions it serves included (its children, which its
    /// own end leaves running): a server gone away. No new connection
    /// first: sshfs would reconnect at once, its session not among those
    /// found.
    fn stop(&mut self) {
        let sessions = self.descendants();
        let _ = self.child.kill();
        signal("-9", &sessions);
        let _ = self.child.wait();
    }

    /// Stop it as a connection lost stops what runs for it: its sessions'
    /// processes hung up (rsync then keeps the file it was writing), then
    /// what is left ended.
    fn stop_gently(&mut self) {
        let sessions = self.descendants();
        let _ = self.child.kill();
        signal("-HUP", &sessions);
        let deadline = Instant::now() + Duration::from_secs(10);
        while sessions
            .iter()
            .any(|pid| Path::new(&format!("/proc/{pid}")).exists())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        signal("-9", &sessions);
        let _ = self.child.wait();
    }

    /// Start it again on its port, as it was.
    fn start_again(&mut self) {
        self.child = OwnSshd::spawn(&self.command, self.port);
    }
}

impl Drop for OwnSshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A folder of what a paste holds: small files of several modes and times,
/// a folder of folders.
fn small_files(root: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(root.join("a folder/deeper")).unwrap();
    for (i, mode) in [0o644, 0o755, 0o600, 0o444]
        .into_iter()
        .cycle()
        .take(24)
        .enumerate()
    {
        let path = root.join(format!("it's {i:02}.txt"));
        std::fs::write(&path, format!("file {i}").repeat(100)).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i as u64))
            .unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    std::fs::write(root.join("a folder/deeper/inside.txt"), "inside").unwrap();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    entries
}

/// `copy` holds what `src` does, as a copy keeps it: names, bytes, modes
/// and files' times, all the way down.
fn same_tree(src: &Path, copy: &Path) {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (
        std::fs::symlink_metadata(src).unwrap(),
        std::fs::symlink_metadata(copy).unwrap(),
    );
    assert_eq!(a.mode(), b.mode(), "{}", copy.display());
    if a.is_file() {
        assert_eq!(std::fs::read(src).unwrap(), std::fs::read(copy).unwrap());
        assert_eq!(a.mtime(), b.mtime(), "{}", copy.display());
        return;
    }
    for entry in std::fs::read_dir(src).unwrap() {
        let name = entry.unwrap().file_name();
        same_tree(&src.join(&name), &copy.join(&name));
    }
}

/// `copy` holds what `src` does, its names, bytes and files' times, all
/// the way down: what a server that keeps no modes (`rclone serve`) keeps.
fn same_files(src: &Path, copy: &Path) {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (
        std::fs::symlink_metadata(src).unwrap(),
        std::fs::symlink_metadata(copy).unwrap(),
    );
    if a.is_file() {
        assert_eq!(std::fs::read(src).unwrap(), std::fs::read(copy).unwrap());
        assert_eq!(a.mtime(), b.mtime(), "{}", copy.display());
        return;
    }
    assert!(b.is_dir(), "{}", copy.display());
    for entry in std::fs::read_dir(src).unwrap() {
        let name = entry.unwrap().file_name();
        same_files(&src.join(&name), &copy.join(&name));
    }
}

/// Wait until `pane` lists `expected`, sorted; past `seconds`, fail
/// saying what it lists.
fn until_shown(
    app: &App,
    pane: skwir_core::workspace::PaneId,
    seconds: u64,
    expected: &[String],
    what: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.pump();
        let shown = names_shown(app, pane);
        if shown == expected {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: it shows {shown:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The names `pane` lists, sorted.
fn names_shown(app: &App, pane: skwir_core::workspace::PaneId) -> Vec<String> {
    let ws = app.workspace.read();
    let mut names: Vec<_> = ws
        .pane(pane)
        .unwrap()
        .listing
        .iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The names of `entries`, sorted.
fn names_of(entries: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<_> = entries
        .iter()
        .map(|e| e.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn moves(entries: &[PathBuf], dest: &Path) -> skwir_core::op::Plan {
    skwir_core::op::Plan::with_steps(
        "move",
        entries
            .iter()
            .map(|e| {
                Box::new(skwir_core::op::Move::new(
                    skwir_core::op::EntryRef::from_path(e).unwrap(),
                    dest.to_path_buf(),
                )) as Box<dyn skwir_core::op::Operation>
            })
            .collect(),
    )
}

fn copies(entries: &[PathBuf], dest: &Path) -> skwir_core::op::Plan {
    skwir_core::op::Plan::with_steps(
        "copy",
        entries
            .iter()
            .map(|e| {
                Box::new(skwir_core::op::Copy {
                    entry: skwir_core::op::EntryRef::from_path(e).unwrap(),
                    dest_dir: dest.to_path_buf(),
                    new_name: None,
                    preserve_attributes: true,
                }) as Box<dyn skwir_core::op::Operation>
            })
            .collect(),
    )
}

#[test]
fn copies_and_moves_to_and_from_an_sftp_server_go_through_rsync_in_batches() {
    let sshfs = Sshfs {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        ..Sshfs::default()
    };
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rsync");
    let Some(sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(&served).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());

    // Up: one rsync for the folder, staged, one for the files, placed
    // beside, into a folder whose name the server's shell would split.
    let local = base.join("local");
    let entries = small_files(&local);
    let into = mounted.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    app.navigate(pane, &Location::from_path(&into)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(up.applied, entries.len());
    let copied_names = names_of(&entries);
    // Copied beside the mount, no watch saw them: the pane reads the
    // folder again, and sshfs does not show it as it was.
    until_shown(
        &app,
        pane,
        10,
        &copied_names,
        "the pane shows what rsync copied",
    );
    assert_eq!(
        sshfs.rsync().made(),
        2,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_tree(
            e,
            &served.join("it's a folder").join(e.file_name().unwrap()),
        );
    }
    let left_on_server: Vec<_> = std::fs::read_dir(served.join("it's a folder"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().starts_with(".skwir-copy-"))
        .collect();
    assert!(left_on_server.is_empty(), "nothing staged left");

    // Down: what is on the server, through its mounted folder.
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(
        sshfs.rsync().made(),
        4,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_tree(e, &down.join(e.file_name().unwrap()));
    }

    // Moved up: copied as copies are, the originals removed after; and
    // back down by Undo, the same way.
    let moved_into = mounted.join("moved here");
    std::fs::create_dir(&moved_into).unwrap();
    app.navigate(pane, &Location::from_path(&moved_into))
        .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    // A name sftp reads as a pattern, said as itself.
    let odd = down.join(r#"a "quoted" *?[glob].txt"#);
    std::fs::write(&odd, "odd").unwrap();
    let mut here: Vec<PathBuf> = entries
        .iter()
        .map(|e| down.join(e.file_name().unwrap()))
        .collect();
    here.push(odd.clone());
    let moved = app
        .executor
        .execute_blocking(&moves(&here, &moved_into), app.lifetime());
    assert!(moved.error.is_none(), "{:?}", moved.error);
    assert_eq!(sshfs.rsync().made(), 6, "moved as copied, by rsync");
    assert_eq!(
        std::fs::read_dir(&down).unwrap().count(),
        0,
        "the originals removed"
    );
    for e in &entries {
        same_tree(e, &served.join("moved here").join(e.file_name().unwrap()));
    }
    let mut moved_names = copied_names.clone();
    moved_names.push(odd.file_name().unwrap().to_string_lossy().into_owned());
    moved_names.sort();
    until_shown(
        &app,
        pane,
        10,
        &moved_names,
        "the pane shows what was moved there",
    );
    // Back down: the originals on the server removed together by rsync,
    // the folder's tree renamed aside first.
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert_eq!(sshfs.rsync().made(), 8, "moved back by rsync too");
    assert_eq!(sshfs.rsync().removed(), 1, "the originals removed by rsync");
    assert_eq!(
        std::fs::read_dir(served.join("moved here"))
            .unwrap()
            .count(),
        0
    );
    for e in &entries {
        same_tree(e, &down.join(e.file_name().unwrap()));
    }
    assert_eq!(std::fs::read(&odd).unwrap(), b"odd");
    // sshfs shows them gone: the pane reads the folder again, and their
    // names are free at once through the mount (sshfs kept them 20 s).
    until_shown(&app, pane, 10, &[], "the pane shows the originals gone");
    std::fs::File::create_new(moved_into.join("it's 00.txt")).unwrap();
    std::fs::create_dir(moved_into.join("a folder")).unwrap();
    until(&app, 3, "a removed name is gone through the mount", || {
        std::fs::symlink_metadata(moved_into.join("it's 01.txt")).is_err()
            && std::fs::symlink_metadata(moved_into.join("a folder/deeper")).is_err()
    });
    std::fs::remove_file(moved_into.join("it's 00.txt")).unwrap();
    std::fs::remove_dir(moved_into.join("a folder")).unwrap();

    // Watched, as GVfs's trash watches the top of every mount: a watch is
    // no use, and Eject lets go of it all the same.
    let watching = rustix::fs::inotify::init(rustix::fs::inotify::CreateFlags::CLOEXEC).unwrap();
    rustix::fs::inotify::add_watch(&watching, &point, rustix::fs::inotify::WatchFlags::CREATE)
        .unwrap();
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    let refused: Vec<_> = app
        .take_notifications()
        .into_iter()
        .filter(|n| n.text.contains("not disconnected"))
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
    drop(watching);
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Copies made on a server are taken back by Undo and deleted for good
/// together, by rsync in one run, as the pane and sshfs show at once: a
/// network folder a program mounts keeps no trash, so Undo removes a copy
/// while it is as it was made, and Redo copies it again.
#[test]
fn copies_on_a_server_are_undone_and_deleted_together() {
    let sshfs = Sshfs {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        ..Sshfs::default()
    };
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("undone");
    let Some(sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(served.join("up")).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let up = mounted.join("up");
    app.navigate(pane, &Location::from_path(&up)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));

    let local = base.join("local");
    let entries = small_files(&local);
    let copied = app
        .executor
        .execute_blocking(&copies(&entries, &up), app.lifetime());
    assert!(copied.error.is_none(), "{:?}", copied.error);
    until_shown(
        &app,
        pane,
        10,
        &names_of(&entries),
        "the pane shows the copies",
    );

    // Undo: taken back together, by rsync.
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert_eq!(undone.applied, entries.len());
    assert_eq!(sshfs.rsync().removed(), 1, "taken back in one run");
    assert_eq!(std::fs::read_dir(served.join("up")).unwrap().count(), 0);
    until_shown(&app, pane, 10, &[], "the pane shows them gone");
    std::fs::File::create_new(up.join("it's 00.txt")).unwrap();
    std::fs::remove_file(up.join("it's 00.txt")).unwrap();

    // Redo: copied again.
    let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(redone.error.is_none(), "{:?}", redone.error);
    for e in &entries {
        same_tree(e, &served.join("up").join(e.file_name().unwrap()));
    }

    // Deleted for good: together too.
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| up.join(e.file_name().unwrap()))
        .collect();
    let for_good = skwir_core::op::Plan::with_steps(
        "delete",
        there
            .iter()
            .map(|e| {
                Box::new(skwir_core::op::Delete {
                    entry: skwir_core::op::EntryRef::from_path(e).unwrap(),
                }) as Box<dyn skwir_core::op::Operation>
            })
            .collect(),
    );
    let deleted = app.executor.execute_blocking(&for_good, app.lifetime());
    assert!(deleted.error.is_none(), "{:?}", deleted.error);
    assert_eq!(deleted.applied, entries.len());
    assert_eq!(sshfs.rsync().removed(), 2, "deleted in one run");
    assert_eq!(std::fs::read_dir(served.join("up")).unwrap().count(), 0);
    until_shown(&app, pane, 10, &[], "the pane shows them deleted");
    std::fs::create_dir(up.join("a folder")).unwrap();
    std::fs::remove_dir(up.join("a folder")).unwrap();

    // Copied within the server: there, by sftp's `cp`, the bytes never
    // here, each given its source's time.
    let again = app
        .executor
        .execute_blocking(&copies(&entries, &up), app.lifetime());
    assert!(again.error.is_none(), "{:?}", again.error);
    let within = mounted.join("within");
    std::fs::create_dir(&within).unwrap();
    let inside = app
        .executor
        .execute_blocking(&copies(&there, &within), app.lifetime());
    assert!(inside.error.is_none(), "{:?}", inside.error);
    assert_eq!(
        sshfs.sftp().made(),
        2,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_tree(e, &served.join("within").join(e.file_name().unwrap()));
    }

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A large upload its server stopped answering partway through keeps what
/// it sent in its staging folder there, and Retry, the server back, goes
/// on from it, after reading back its last part: a byte marked at its
/// start since is in the copy. A download keeps what came, here. The
/// test's server is stopped once part of the file is across, its
/// sessions with it.
#[test]
fn an_upload_cut_short_by_its_server_goes_on_from_where_it_stopped() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("resumed");
    let Some(mut sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(served.join("up")).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let up = mounted.join("up");

    // 192 MiB that differ along their length.
    let local = base.join("local");
    std::fs::create_dir_all(&local).unwrap();
    let big = local.join("it's big.bin");
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    let bytes: Vec<u8> = (0..192u64 << 20)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    std::fs::write(&big, &bytes).unwrap();
    drop(bytes);

    let plan = copies(std::slice::from_ref(&big), &up);
    app.executor.execute(plan.clone(), app.lifetime().child());
    // On the server, in the staging folder, once 24 MiB are there.
    let staged = || {
        std::fs::read_dir(served.join("up"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".skwir-copy-"))
            .flat_map(|e| std::fs::read_dir(e.path()).unwrap().flatten())
            .find(|e| e.file_name() != "lock")
            .map(|e| e.path())
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while staged()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_none_or(|m| m.len() < 24 << 20)
    {
        assert!(Instant::now() < deadline, "the upload never began");
        std::thread::sleep(Duration::from_millis(2));
    }
    sshd.stop();
    until(&app, 120, "the upload stopped", || !app.executor.is_busy());
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    let partial = staged().expect("what it sent is kept");
    let kept = std::fs::metadata(&partial).unwrap().len();
    assert!((8 << 20..192 << 20).contains(&kept), "{kept} bytes kept");
    {
        use std::os::unix::fs::FileExt;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&partial)
            .unwrap()
            .write_all_at(b"X", 0)
            .unwrap();
    }

    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let copy = served.join("up").join("it's big.bin");
    let (sent, copied) = (std::fs::read(&big).unwrap(), std::fs::read(&copy).unwrap());
    assert_eq!(copied.len(), sent.len());
    assert_eq!(copied[0], b'X', "gone on from what it had sent");
    assert!(copied[1..] == sent[1..], "the rest as sent");
    assert_eq!(
        std::fs::read_dir(served.join("up")).unwrap().count(),
        1,
        "nothing staged left there"
    );
    drop((sent, copied));

    // Down: kept on this machine, its last part read back from the
    // server. A file the server has that this machine never sent: what
    // went up through the mount may still be in its cache here, read
    // whole whatever the server.
    let there = served.join("there").join("it's big.bin");
    std::fs::create_dir(served.join("there")).unwrap();
    std::fs::copy(&copy, &there).unwrap();
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let plan = copies(&[mounted.join("there").join("it's big.bin")], &down);
    app.executor.execute(plan.clone(), app.lifetime().child());
    let staged = || {
        std::fs::read_dir(&down)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".skwir-copy-"))
            .flat_map(|e| std::fs::read_dir(e.path()).unwrap().flatten())
            .find(|e| e.file_name() != "lock")
            .map(|e| e.path())
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while staged()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_none_or(|m| m.len() < 24 << 20)
    {
        assert!(Instant::now() < deadline, "the download never began");
        std::thread::sleep(Duration::from_millis(2));
    }
    sshd.stop();
    until(&app, 120, "the download stopped", || {
        !app.executor.is_busy()
    });
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    let partial = staged().expect("what came is kept");
    {
        use std::os::unix::fs::FileExt;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&partial)
            .unwrap()
            .write_all_at(b"Y", 0)
            .unwrap();
    }
    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let (there, came) = (
        std::fs::read(&there).unwrap(),
        std::fs::read(down.join("it's big.bin")).unwrap(),
    );
    assert_eq!(came.len(), there.len());
    assert_eq!(came[0], b'Y', "gone on from what had come");
    assert!(came[1..] == there[1..], "the rest as it is there");
    assert_eq!(std::fs::read_dir(&down).unwrap().count(), 1);

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A file under `root` whose name begins so, and its length.
fn found(root: &Path, begins: &str) -> Option<(PathBuf, u64)> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if entry.file_name().to_string_lossy().starts_with(begins) && meta.is_file() {
            return Some((path, meta.len()));
        }
        if meta.is_dir()
            && let Some(found) = found(&path, begins)
        {
            return Some(found);
        }
    }
    None
}

/// A folder copied up by rsync, its server gone partway through its
/// second file, keeps what the batch made in its staging folder there,
/// and rsync the file it was writing; Retry, the server back, goes on in
/// it: the file finished stays (a byte marked since, its time given back,
/// is still there: rsync passes it over), the one cut short is the basis
/// rsync goes on from, gone after. Down the same, kept on this machine.
#[test]
fn a_folder_cut_short_by_its_server_goes_on_by_rsync() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    if skwir_core::os_path::find_program("rsync").is_none() {
        eprintln!("skipped: rsync is not installed");
        return;
    }
    let base = fixture("resumed-folder");
    let Some(mut sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(served.join("up")).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let up = mounted.join("up");

    // 16 and 160 MiB that differ along their length, rsync's order.
    let folder = base.join("local").join("it's a folder");
    std::fs::create_dir_all(&folder).unwrap();
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    for (name, mib) in [("a first.bin", 16u64), ("z second.bin", 160)] {
        let bytes: Vec<u8> = (0..mib << 20)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        std::fs::write(folder.join(name), &bytes).unwrap();
    }
    // The finished file in the batch's folder, marked, its time given back.
    let mark = |staging_in: &Path, byte: u8| {
        let (finished, _) = found(staging_in, "a first.bin").expect("finished, kept");
        let when = std::fs::metadata(&finished).unwrap().modified().unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&finished)
            .unwrap();
        std::os::unix::fs::FileExt::write_all_at(&file, &[byte], 0).unwrap();
        file.set_modified(when).unwrap();
    };
    let partial = |staging_in: &Path| {
        std::fs::read_dir(staging_in)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".skwir-copy-"))
            .flat_map(|e| std::fs::read_dir(e.path()).unwrap().flatten())
            .filter(|e| e.file_name().to_string_lossy().starts_with("partial-"))
            .find_map(|e| found(&e.path(), "z second.bin"))
    };
    let underway = |staging_in: &Path| {
        let deadline = Instant::now() + Duration::from_secs(120);
        while found(staging_in, ".z second.bin.").is_none_or(|(_, len)| len < 24 << 20) {
            assert!(Instant::now() < deadline, "the copy never began");
            std::thread::sleep(Duration::from_millis(2));
        }
    };

    let plan = copies(std::slice::from_ref(&folder), &up);
    app.executor.execute(plan.clone(), app.lifetime().child());
    underway(&served.join("up"));
    sshd.stop_gently();
    until(&app, 120, "the upload stopped", || !app.executor.is_busy());
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    let (_, kept) = partial(&served.join("up")).expect("rsync kept what it wrote");
    assert!((8 << 20..160 << 20).contains(&kept), "{kept} bytes kept");
    mark(&served.join("up"), b'X');
    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let copy = served.join("up").join("it's a folder");
    let first = std::fs::read(copy.join("a first.bin")).unwrap();
    let sent = std::fs::read(folder.join("a first.bin")).unwrap();
    assert!(
        first[0] == b'X' && first[1..] == sent[1..],
        "finished, kept"
    );
    same_bytes(&folder.join("z second.bin"), &copy.join("z second.bin"));
    assert_eq!(
        std::fs::read_dir(served.join("up")).unwrap().count(),
        1,
        "nothing staged left there"
    );

    // Down: kept on this machine.
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let plan = copies(&[up.join("it's a folder")], &down);
    app.executor.execute(plan.clone(), app.lifetime().child());
    underway(&down);
    sshd.stop();
    until(&app, 120, "the download stopped", || {
        !app.executor.is_busy()
    });
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    let (_, kept) = partial(&down).expect("rsync kept what came");
    assert!((8 << 20..160 << 20).contains(&kept), "{kept} bytes kept");
    mark(&down, b'Y');
    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let came = std::fs::read(down.join("it's a folder").join("a first.bin")).unwrap();
    assert!(came[0] == b'Y' && came[1..] == first[1..], "finished, kept");
    same_bytes(
        &folder.join("z second.bin"),
        &down.join("it's a folder").join("z second.bin"),
    );
    assert_eq!(std::fs::read_dir(&down).unwrap().count(), 1);

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Copies within one rclone mount Skwir made go by rclone itself, on the
/// server (WebDAV's `COPY`), through the mount's remote control on its
/// socket: a folder and a large file, whole there, the mount showing them.
#[test]
fn rclone_copies_within_its_mount_on_the_server() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone-within");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("a folder/inside")).unwrap();
    std::fs::write(served_root.join("a folder/one.txt"), "one").unwrap();
    std::fs::write(served_root.join("a folder/inside/two.txt"), "two").unwrap();
    std::fs::write(served_root.join("large.bin"), vec![7u8; 32 << 20]).unwrap();
    std::fs::create_dir(served_root.join("copies")).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let address = Location::parse(&format!("dav://127.0.0.1:{}/", served.port));
    app.navigate(pane, &address).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let before = skwir_plugin_remote::rclone_copy::shared().made();
    let r = app.executor.execute_blocking(
        &copies(
            &[mounted.join("a folder"), mounted.join("large.bin")],
            &mounted.join("copies"),
        ),
        app.lifetime(),
    );
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(
        skwir_plugin_remote::rclone_copy::shared().made() > before,
        "by rclone, on the server"
    );
    same_bytes(
        &served_root.join("large.bin"),
        &served_root.join("copies/large.bin"),
    );
    assert_eq!(
        std::fs::read_to_string(served_root.join("copies/a folder/inside/two.txt")).unwrap(),
        "two"
    );
    assert_eq!(
        std::fs::read_to_string(mounted.join("copies/a folder/one.txt")).unwrap(),
        "one",
        "the mount shows them"
    );
    assert_eq!(
        names_in(&served_root.join("copies")),
        ["a folder", "large.bin"],
        "nothing staged left"
    );

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A copy onto a mount that sends what is written after it is closed
/// (rclone's `full` cache, chosen in the server's Mount Options) is done
/// when the server has it (ADR-102): the plan ends after the server has
/// every file it wrote, its last ones too (queued by rclone only once their
/// release reaches it, after `close` returned), a move's original goes only
/// then, and Disconnect sends what another program wrote there before
/// letting go. Read on the server the moment each ends: on `full`, rclone
/// sends a file 5 s after its close.
#[test]
fn a_copy_onto_a_mount_that_sends_later_ends_once_the_server_has_it() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone-sends-later");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("copies")).unwrap();
    let local = base.join("local");
    std::fs::create_dir_all(local.join("a folder/l'été")).unwrap();
    std::fs::write(local.join("a note’s draft.txt"), "a note").unwrap();
    std::fs::write(local.join("a folder/l'été/two.txt"), "two").unwrap();
    std::fs::write(local.join("moved away.txt"), "moved").unwrap();
    // Several, each its own step: the plan's last close comes just before
    // it ends.
    let several: Vec<String> = (0..12).map(|n| format!("several {n}’s.txt")).collect();
    for name in &several {
        std::fs::write(local.join(name), name).unwrap();
    }
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let top = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let full = skwir_plugin_remote::options::MountOptions {
        cache: Some("full".into()),
        ..skwir_plugin_remote::options::MountOptions::default()
    };
    skwir_plugin_remote::options::keep(&app, &top, &full).unwrap();
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse(&top.to_string()))
        .unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let control = skwir_plugin_remote::uploads::control_of(&point).unwrap();
    assert_eq!(
        skwir_plugin_remote::uploads::cache_mode(&control, &CancelToken::new()).unwrap(),
        "full",
        "mounted with the cache chosen"
    );
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();

    let mut entries = vec![local.join("a note’s draft.txt"), local.join("a folder")];
    entries.extend(several.iter().map(|name| local.join(name)));
    let r = app
        .executor
        .execute_blocking(&copies(&entries, &mounted.join("copies")), app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(
        read(&served_root.join("copies/a note’s draft.txt")),
        "a note"
    );
    assert_eq!(
        read(&served_root.join("copies/a folder/l'été/two.txt")),
        "two"
    );
    for name in &several {
        assert_eq!(
            &read(&served_root.join("copies").join(name)),
            name,
            "on the server once the plan ended"
        );
    }

    let r = app.executor.execute_blocking(
        &moves(&[local.join("moved away.txt")], &mounted.join("copies")),
        app.lifetime(),
    );
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(read(&served_root.join("copies/moved away.txt")), "moved");
    assert!(
        !local.join("moved away.txt").exists(),
        "its original went once the server had the copy"
    );

    // Written by another program, waiting to be sent (queued once rclone
    // has its release): sent first.
    std::fs::write(mounted.join("copies/written aside.txt"), "aside").unwrap();
    until_queued(&control);
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 60, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    assert_eq!(
        read(&served_root.join("copies/written aside.txt")),
        "aside",
        "sent before it was let go of"
    );
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// What another program left on a mount that sends later is said on its
/// place while it waits (ADR-102), asked when the window comes back
/// (`App::refresh_remote`, `Known::looked_at`) and then while some waits,
/// gone once the server has it.
#[test]
fn what_another_program_left_to_send_is_said_on_its_place_until_sent() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone-left-to-send");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let top = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let full = skwir_plugin_remote::options::MountOptions {
        cache: Some("full".into()),
        ..skwir_plugin_remote::options::MountOptions::default()
    };
    skwir_plugin_remote::options::keep(&app, &top, &full).unwrap();
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse(&top.to_string()))
        .unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let said = || {
        app.registry
            .places
            .all()
            .into_iter()
            .find(|place| place.location == Location::from_path(&point))
            .and_then(|place| place.busy)
    };
    assert_eq!(said(), None);

    // Another program writes there: rclone sends it 5 s after its close,
    // here once the test lets it go (`held_in_queue`), however long the
    // window takes to say so on a machine under load.
    std::fs::write(mounted.join("l'été — written aside.txt"), "aside").unwrap();
    let at = skwir_plugin_remote::uploads::control_of(&point).expect("its remote control");
    let waiting = held_in_queue(&at);
    app.refresh_remote();
    until(&app, 60, "said on its place", || {
        said().as_deref() == Some("Sending 1 file to the server…")
    });
    let_go_in_queue(&at, waiting);
    until(&app, 60, "no more said once sent", || said().is_none());
    assert_eq!(
        std::fs::read_to_string(served_root.join("l'été — written aside.txt")).unwrap(),
        "aside"
    );

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The upload rclone queued last at the mount whose remote control is
/// `at`, held there an hour on (`vfs/queue-set-expiry`, as a server slow to
/// take it would keep it): its id. Queued once rclone has the file's
/// release, which the kernel sends after `close` returns, on its own time;
/// held well within the 5 s rclone waits before sending it.
fn held_in_queue(at: &skwir_plugin_remote::rc::At) -> u64 {
    use skwir_plugin_remote::rc::call;
    let cancel = CancelToken::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let queue = call(at, "vfs/queue", &serde_json::json!({}), &cancel).unwrap();
        if let Some(id) = queue["queue"][0]["id"].as_u64() {
            let later = serde_json::json!({ "id": id, "expiry": 3600, "relative": false });
            call(at, "vfs/queue-set-expiry", &later, &cancel).unwrap();
            return id;
        }
        assert!(Instant::now() < deadline, "never queued to be sent");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Until rclone's queue at the mount whose remote control is `at` holds an
/// upload: a file closed there reaches it with its release, which the
/// kernel sends after `close` returned, on its own time.
fn until_queued(at: &skwir_plugin_remote::rc::At) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while skwir_plugin_remote::uploads::waiting(at, &CancelToken::new()).unwrap() == 0 {
        assert!(Instant::now() < deadline, "never queued to be sent");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The upload `id` held (`held_in_queue`) sent now.
fn let_go_in_queue(at: &skwir_plugin_remote::rc::At, id: u64) {
    let now = serde_json::json!({ "id": id, "expiry": 0, "relative": false });
    skwir_plugin_remote::rc::call(at, "vfs/queue-set-expiry", &now, &CancelToken::new()).unwrap();
}

/// A text file on a mount that keeps nothing it reads (rclone's `minimal`,
/// which Automatic chooses on the local network) is fetched once for its
/// preview (ADR-101): every provider asked at each stage, the text's
/// first rows, its rows counted and read as the view reads them. Counted
/// at the server, where each fetch is a GET.
#[test]
fn a_preview_of_a_file_on_a_mount_fetches_it_once() {
    use skwir_core::inspection::InspectRequest;
    use skwir_core::preview::{HeldSlot, PreviewRequest};
    use skwir_platform::text_rows;
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone-preview-once");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let text: String = (1..=600).fold(String::new(), |mut text, n| {
        use std::fmt::Write as _;
        let _ = writeln!(text, "line {n} of a note");
        text
    });
    std::fs::write(served_root.join("a note.txt"), &text).unwrap();
    let served = Served::start("webdav", &served_root, &["-v"]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    // Every plugin that previews, as the window has them, with rclone alone
    // to mount.
    for plugin in skwir::plugins()
        .into_iter()
        .filter(|plugin| plugin.id() != "remote")
    {
        app.plugins.add(plugin, &app);
    }
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let address = Location::parse(&format!("dav://127.0.0.1:{}/", served.port));
    app.navigate(pane, &address).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let log = served_root.with_extension("webdav.log");
    let fetched = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("a note.txt: GET from"))
            .count()
    };
    let before = fetched();

    // As the panel's worker asks, in its order (`Shell::preview_request`).
    let path = mounted.join("a note.txt");
    let request = PreviewRequest {
        entry: skwir_core::op::EntryRef::from_path(&path).unwrap(),
        mime: Some("text/plain".into()),
        page: 1,
        seconds: None,
        max_side: 1024,
        text_size: None,
        fit: None,
        held: HeldSlot::default(),
    };
    let (previews, cancel) = (&app.previews, CancelToken::new());
    assert!(previews.converted(&request, &cancel).unwrap().is_none());
    let mut view = InspectRequest::default();
    let shown =
        skwir_ui::preview::inspect_shown(previews, &request, &mut view, &request, false, &cancel)
            .unwrap();
    assert!(shown.is_none(), "no provider takes plain text");
    assert!(!previews.media(&request));
    assert!(previews.open_document(&request, &cancel).unwrap().is_none());
    assert!(previews.render(&request, &cancel).unwrap().is_none());
    let held = request.held().unwrap();
    assert!(skwir_platform::inspection::is_text(held.head()));
    let plain = skwir_platform::inspection::plain(&held, &cancel).unwrap();
    // The view: its rows counted, a chunk read.
    let file = plain.text.unwrap();
    let form = text_rows::Form::read(&file, &cancel).unwrap();
    let index = parking_lot::RwLock::new(text_rows::Index::new(form));
    text_rows::count(&file, &index, &cancel, || {}).unwrap();
    assert_eq!(index.read().lines(), 600);
    let rows = text_rows::read(&file, form, (0, text_rows::origin(form)), 0..300, &cancel).unwrap();
    assert_eq!(rows[299].text, "line 300 of a note");

    assert_eq!(fetched() - before, 1, "fetched once ({})", log.display());
    drop(request);
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The Nearby place (ADR-084) lists what Avahi and wsdd find, each server
/// once, leading to its address; let go of, the wsdd it started ends and
/// its connection to Avahi is closed. Run by `tools/test/nearby.sh`, in
/// namespaces of its own (`just remote-check`): an SMB and a WebDAV
/// server announced by an Avahi of the test's own, with `BURST` FTP
/// servers more than zbus queues signals for (64: what the browsers say
/// first comes at once, read while a resolve waits), a Windows PC by wsdd
/// across a veth pair.
#[test]
fn nearby_lists_what_avahi_and_wsdd_find() {
    if std::env::var_os("SKWIR_TEST_NEARBY").is_none() {
        eprintln!("skipped: run by tools/test/nearby.sh");
        return;
    }
    let base = fixture("nearby");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins.add(
        Arc::new(RemotePlugin::with(Remotes::with(Vec::new()))),
        &app,
    );
    let nearby = skwir_plugin_remote::nearby::location();
    until(&app, 10, "the Network section offers it", || {
        app.registry
            .places
            .find(|p| p.group == "network" && p.location == nearby)
            .is_some()
    });
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &nearby).unwrap();
    let names = ["Test Files (WebDAV)", "Test NAS (SMB)", "WINPC (SMB)"].map(str::to_owned);
    let burst: usize = std::env::var("BURST").map_or(0, |n| n.parse().unwrap());
    let mut all: Vec<String> = (1..=burst).map(|n| format!("Burst {n:02} (FTP)")).collect();
    all.extend(names.iter().cloned());
    all.sort();
    until_shown(&app, pane, 60, &all, "the servers announced");
    let leads: Vec<(String, String)> = {
        let ws = app.workspace.read();
        let shown = ws.pane(pane).unwrap();
        let open = shown.open.clone().expect("open");
        let mut leads: Vec<(String, String)> = shown
            .listing
            .iter()
            .map(|e| {
                assert!(e.is_place(), "a place, no file");
                let to = open.location_of(e.id).expect("leads somewhere");
                (e.name.to_string_lossy().into_owned(), to.to_string())
            })
            .filter(|(name, _)| !name.starts_with("Burst "))
            .collect();
        leads.sort();
        leads
    };
    assert_eq!(
        leads,
        [
            (names[0].clone(), "dav://10.89.0.1:8080/dav".to_owned()),
            (names[1].clone(), "smb://10.89.0.1:445/".to_owned()),
            (names[2].clone(), "smb://10.89.0.2/".to_owned()),
        ]
    );
    // One selected, Add as Server… adds it, its form filled in with what it
    // announced; several, nothing.
    let rows: Vec<(String, skwir_core::entry::EntryId)> = {
        let ws = app.workspace.read();
        let shown = ws.pane(pane).unwrap();
        shown
            .listing
            .iter()
            .map(|e| (e.name.to_string_lossy().into_owned(), e.id))
            .collect()
    };
    let row = |name: &str| rows.iter().find(|(n, _)| n == name).unwrap().1;
    let menu = |ids: Vec<skwir_core::entry::EntryId>| {
        app.workspace
            .write()
            .pane_mut(pane)
            .unwrap()
            .selection
            .set(ids);
        let Outcome::Value(menu) = app
            .dispatch(&Invocation::with_args(
                "remote.nearby_menu",
                &serde_json::json!({ "pane": pane }),
            ))
            .unwrap()
        else {
            panic!("a menu");
        };
        menu["items"].as_array().unwrap().clone()
    };
    let items = menu(vec![row(&names[0])]);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["title"], "Add as Server…");
    assert_eq!(items[0]["command"]["name"], "remote.add_server");
    assert_eq!(
        items[0]["command"]["args"],
        serde_json::json!({
            "kind": "address:dav",
            "options": { "host": "10.89.0.1", "port": "8080", "path": "dav", "title": "Test Files" },
        })
    );
    let items = menu(vec![row(&names[2])]);
    assert_eq!(
        items[0]["command"]["args"],
        serde_json::json!({ "kind": "address:smb", "options": { "host": "10.89.0.2", "title": "WINPC" } })
    );
    assert!(menu(vec![row(&names[0]), row(&names[1])]).is_empty());
    // Let go of: nothing is asked of the network any more.
    let discovering = || {
        std::fs::read_dir("/proc").unwrap().flatten().any(|p| {
            std::fs::read(p.path().join("cmdline"))
                .is_ok_and(|c| c.windows(11).any(|w| w == b"--discovery"))
        })
    };
    assert!(discovering(), "wsdd started to find PCs");
    assert!(on_the_system_bus(), "Avahi is asked over the system bus");
    app.navigate(pane, &Location::from_path(&base)).unwrap();
    // As the window's view does at its next draw: what it left is let go.
    app.keep_locations(pane, &[]);
    until(&app, 20, "the wsdd it started ended", || !discovering());
    // Its browsers freed, its connection closed: Avahi browses no more.
    until(&app, 20, "its connection to Avahi was closed", || {
        !on_the_system_bus()
    });
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The wsdd processes discovering for the Skwir `pid`: those told to
/// listen on a socket it names (`wsdd-<pid>-…`), a zombie left out.
fn wsdd_of(pid: u32) -> Vec<u32> {
    let mark = format!("wsdd-{pid}-");
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|process| {
            let found = process.file_name().to_str()?.parse::<u32>().ok()?;
            let line = std::fs::read(process.path().join("cmdline")).ok()?;
            line.split(|b| *b == 0)
                .any(|arg| {
                    Path::new(&*String::from_utf8_lossy(arg))
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(&mark))
                })
                .then_some(found)
        })
        .collect()
}

/// The sockets of the runtime folder Skwir's wsdd listen on for the
/// Skwir `pid` (`wsdd-<pid>-…`).
fn wsdd_sockets_of(pid: u32) -> Vec<PathBuf> {
    let folder = skwir_core::xdg::runtime_dir().unwrap().join("skwir");
    let mark = format!("wsdd-{pid}-");
    std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(&mark))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the process `pid` runs: there, and no zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(") ")
            .is_some_and(|(_, rest)| !rest.starts_with('Z'))
    })
}

/// A Skwir killed while the Nearby place is shown (a crash: nothing stops
/// what it started) leaves no wsdd discovering: the system ends it with
/// Skwir. What one left before (its socket, a wsdd still listening there,
/// as a Skwir without that would) is swept once Nearby is shown again.
/// Run by `tools/test/nearby.sh`, the Skwir killed this test run again.
#[test]
fn a_wsdd_skwir_started_ends_with_skwir_killed() {
    const OWNER: &str = "SKWIR_TEST_WSDD_OWNER";
    if std::env::var_os("SKWIR_TEST_NEARBY").is_none() {
        eprintln!("skipped: run by tools/test/nearby.sh");
        return;
    }
    let nearby = skwir_plugin_remote::nearby::location();
    let show = |app: &App| {
        app.plugins
            .add(Arc::new(RemotePlugin::with(Remotes::with(Vec::new()))), app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &nearby).unwrap();
        // Started, and listening: its socket made.
        until(app, 20, "its wsdd discovering", || {
            let me = std::process::id();
            !wsdd_of(me).is_empty() && !wsdd_sockets_of(me).is_empty()
        });
        pane
    };
    if std::env::var_os(OWNER).is_some() {
        // The Skwir killed: Nearby shown, then waiting to be killed.
        let base = fixture("wsdd-killed");
        let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
        show(&app);
        println!("discovering");
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut [0]);
        return;
    }
    let mut owner = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_wsdd_skwir_started_ends_with_skwir_killed",
            "--nocapture",
        ])
        .env(OWNER, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut said = std::io::BufReader::new(owner.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("discovering") {
        line.clear();
        let read = std::io::BufRead::read_line(&mut said, &mut line).unwrap();
        assert!(read > 0, "the Skwir to kill ended first");
    }
    let killed = owner.id();
    let left = wsdd_of(killed);
    assert_eq!(left.len(), 1, "its wsdd: {left:?}");
    owner.kill().unwrap();
    owner.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while left.iter().any(|&pid| alive(pid)) {
        assert!(Instant::now() < deadline, "its wsdd outlived it: {left:?}");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Its socket stays, and a wsdd still listening beside it, as one a
    // Skwir before left: both swept once Nearby is shown here.
    assert_eq!(
        wsdd_sockets_of(killed).len(),
        1,
        "a killed wsdd removes nothing"
    );
    let before_socket = skwir_core::xdg::runtime_dir()
        .unwrap()
        .join(format!("skwir/wsdd-{killed}-0.sock"));
    let mut before = Command::new("wsdd")
        .args(["--no-host", "--discovery", "--listen"])
        .arg(&before_socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !before_socket.exists() {
        assert!(Instant::now() < deadline, "the wsdd left never listened");
        std::thread::sleep(Duration::from_millis(20));
    }
    let base = fixture("wsdd-sweeps");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let pane = show(&app);
    let deadline = Instant::now() + Duration::from_secs(20);
    while before.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the wsdd left was not stopped");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        wsdd_sockets_of(killed),
        Vec::<PathBuf>::new(),
        "their sockets swept"
    );
    app.navigate(pane, &Location::from_path(&base)).unwrap();
    app.keep_locations(pane, &[]);
    until(&app, 20, "its own wsdd ended", || {
        wsdd_of(std::process::id()).is_empty()
    });
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
    let _ = std::fs::remove_dir_all(
        dirs::home_dir()
            .unwrap()
            .join(".cache/skwir-tests/remote")
            .join(format!("wsdd-killed-{killed}")),
    );
}

/// Whether this process has a connection on the system bus, as its bus
/// says (`dbus-send`, on the `PATH` of `tools/test/nearby.sh`).
fn on_the_system_bus() -> bool {
    let ask = |args: &[&str]| {
        let out = Command::new("dbus-send")
            .args([
                "--system",
                "--print-reply",
                "--dest=org.freedesktop.DBus",
                "/org/freedesktop/DBus",
            ])
            .args(args)
            .output()
            .expect("dbus-send");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let listed = ask(&["org.freedesktop.DBus.ListNames"]);
    let pid = std::process::id().to_string();
    listed
        .lines()
        .filter_map(|line| line.trim().strip_prefix("string \"")?.strip_suffix('"'))
        .filter(|name| name.starts_with(':'))
        .any(|name| {
            let of = ask(&[
                "org.freedesktop.DBus.GetConnectionUnixProcessID",
                &format!("string:{name}"),
            ]);
            of.split_whitespace().last() == Some(pid.as_str())
        })
}

/// A file copied down from a server through its mount proves it fast
/// (ADR-077: Automatic's Small files, where it is not known near), and
/// the plugin words the mount again at the plan's end.
#[test]
fn a_copy_down_proves_its_server_fast() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("proved-fast");
    let Some(sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(&served).unwrap();
    std::fs::write(served.join("large.bin"), vec![7u8; 32 << 20]).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let remote = skwir_core::fs::reach::of(&mounted).unwrap();
    let left = Unmounted(remote.point.clone());
    assert_eq!(skwir_core::fs::reach::proved_fast(&remote), None);
    // This machine's own server reads faster than `FAST` where nothing
    // else runs; under load, a copy may read slower and prove it slow.
    // Copied down again until one reads that fast (from the server, or
    // the kernel's cache of the mount, as the user's second read would):
    // a copy the test timed at `FAST` or more (the copy itself, which the
    // core times, took no longer) proves it fast at once.
    let deadline = Instant::now() + Duration::from_secs(180);
    for n in 0.. {
        let down = base.join(format!("down {n}"));
        std::fs::create_dir(&down).unwrap();
        let began = Instant::now();
        let r = app
            .executor
            .execute_blocking(&copies(&[mounted.join("large.bin")], &down), app.lifetime());
        let took = began.elapsed();
        assert!(r.error.is_none(), "{:?}", r.error);
        same_bytes(&served.join("large.bin"), &down.join("large.bin"));
        std::fs::remove_dir_all(&down).unwrap();
        let proved = skwir_core::fs::reach::proved_fast(&remote);
        if proved == Some(true) {
            break;
        }
        let speed = f64::from(32u32 << 20) / took.as_secs_f64();
        assert!(
            speed < skwir_core::fs::reach::FAST,
            "32 MiB from this machine's own server at {speed:.0} B/s, proved {proved:?}"
        );
        assert!(
            Instant::now() < deadline,
            "never read fast enough to prove it: {speed:.0} B/s last"
        );
    }

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": remote.point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&remote.point).is_none_or(|r| r.point != remote.point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A folder copied up, its server gone partway (ADR-080, 9): the plan
/// stops, waits for its server, and once the server answers again goes on
/// by itself, no Retry asked; the copy whole, nothing staged left.
#[test]
fn a_copy_its_server_stopped_goes_on_by_itself_once_it_answers() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("goes-on");
    let Some(mut sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(served.join("up")).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.executor.set_server_look(Duration::from_millis(300));
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());

    let folder = base.join("local").join("it's a folder");
    std::fs::create_dir_all(&folder).unwrap();
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    for (name, mib) in [("a first.bin", 16u64), ("z second.bin", 160)] {
        let bytes: Vec<u8> = (0..mib << 20)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        std::fs::write(folder.join(name), &bytes).unwrap();
    }
    let plan = copies(std::slice::from_ref(&folder), &mounted.join("up"));
    app.executor.execute(plan.clone(), app.lifetime().child());
    let deadline = Instant::now() + Duration::from_secs(120);
    while found(&served.join("up"), "z second.bin")
        .or_else(|| found(&served.join("up"), ".z second.bin."))
        .is_none_or(|(_, len)| len < 24 << 20)
    {
        assert!(Instant::now() < deadline, "the copy never began");
        std::thread::sleep(Duration::from_millis(2));
    }
    sshd.stop();
    until(&app, 120, "it stopped, waiting for its server", || {
        app.executor.waits_for_server(plan.id)
    });
    assert!(app.executor.can_retry(plan.id));
    sshd.start_again();
    until(&app, 120, "it went on by itself, through", || {
        !app.executor.can_retry(plan.id) && !app.executor.is_busy()
    });
    let copy = served.join("up").join("it's a folder");
    same_bytes(&folder.join("a first.bin"), &copy.join("a first.bin"));
    same_bytes(&folder.join("z second.bin"), &copy.join("z second.bin"));
    assert_eq!(
        std::fs::read_dir(served.join("up")).unwrap().count(),
        1,
        "nothing staged left there"
    );

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A folder copied up to an SFTP-only server (no rsync) by sftp sessions,
/// the server gone partway through its second file: the batch keeps what
/// it made, the file it was writing too; Retry, the server back, goes on
/// from that file by `reput`, its start read back and never sent again
/// (a byte marked there since stays). Down, Skwir's own copy through the
/// mount goes on so from the file it was writing, kept on this machine.
#[test]
fn a_folder_cut_short_on_an_sftp_only_server_goes_on_from_its_file() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    if skwir_core::os_path::find_program("sftp").is_none() {
        eprintln!("skipped: sftp is not installed");
        return;
    }
    let base = fixture("resumed-sftp");
    let Some(mut sshd) = OwnSshd::start_with(&base, "ForceCommand internal-sftp\n") else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(served.join("up")).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    let up = mounted.join("up");

    // 16 and 160 MiB that differ along their length.
    let folder = base.join("local").join("it's a folder");
    std::fs::create_dir_all(&folder).unwrap();
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    for (name, mib) in [("a first.bin", 16u64), ("z second.bin", 160)] {
        let bytes: Vec<u8> = (0..mib << 20)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        std::fs::write(folder.join(name), &bytes).unwrap();
    }
    // The file under way, in the batch's folder: its first byte marked.
    let underway = |staging_in: &Path| {
        let deadline = Instant::now() + Duration::from_secs(120);
        while found(staging_in, "z second.bin").is_none_or(|(_, len)| len < 24 << 20) {
            assert!(Instant::now() < deadline, "the copy never began");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let mark = |staging_in: &Path, byte: u8| {
        let (cut, len) = found(staging_in, "z second.bin").expect("the file cut short, kept");
        assert!((8 << 20..160 << 20).contains(&len), "{len} bytes kept");
        let file = std::fs::OpenOptions::new().write(true).open(&cut).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&file, &[byte], 0).unwrap();
    };

    let plan = copies(std::slice::from_ref(&folder), &up);
    app.executor.execute(plan.clone(), app.lifetime().child());
    underway(&served.join("up"));
    sshd.stop();
    until(&app, 120, "the upload stopped", || !app.executor.is_busy());
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    assert_eq!(sshfs.rsync().made(), 0, "no rsync to run");
    mark(&served.join("up"), b'Z');
    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let copy = served.join("up").join("it's a folder");
    same_bytes(&folder.join("a first.bin"), &copy.join("a first.bin"));
    let second = std::fs::read(copy.join("z second.bin")).unwrap();
    let sent = std::fs::read(folder.join("z second.bin")).unwrap();
    assert!(
        sent[0] != b'Z' && sent[0] != b'Y',
        "the marks are no source's"
    );
    assert_eq!(second.len(), sent.len());
    assert!(
        second[0] == b'Z' && second[1..] == sent[1..],
        "gone on from, its start never sent again"
    );
    assert_eq!(
        std::fs::read_dir(served.join("up")).unwrap().count(),
        1,
        "nothing staged left there"
    );

    // Down, Skwir's own copy: kept on this machine.
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let plan = copies(&[up.join("it's a folder")], &down);
    app.executor.execute(plan.clone(), app.lifetime().child());
    underway(&down);
    sshd.stop();
    until(&app, 120, "the download stopped", || {
        !app.executor.is_busy()
    });
    assert!(app.executor.can_retry(plan.id), "it failed, to be retried");
    mark(&down, b'Y');
    sshd.start_again();
    let retried = app
        .executor
        .retry_blocking(plan.id, app.lifetime())
        .expect("retried");
    assert!(retried.error.is_none(), "{:?}", retried.error);
    let came = std::fs::read(down.join("it's a folder").join("z second.bin")).unwrap();
    assert!(
        came[0] == b'Y' && came[1..] == sent[1..],
        "gone on from, its start never read again"
    );
    same_bytes(
        &folder.join("a first.bin"),
        &down.join("it's a folder").join("a first.bin"),
    );
    assert_eq!(std::fs::read_dir(&down).unwrap().count(), 1);

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// An sshfs mount the user made, not Skwir: no connection to share, rsync
/// logs in with the user's own ssh (here the test's options), on the port
/// sshfs's command line gave, in the folder the mount table names.
#[test]
fn a_mount_the_user_made_copies_through_rsync_with_their_own_ssh() {
    let Some(sshfs_program) = skwir_core::os_path::find_program("sshfs") else {
        eprintln!("skipped: sshfs is not installed");
        return;
    };
    let base = fixture("theirs");
    let Some(sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(&served).unwrap();
    let point = base.join("their mount");
    std::fs::create_dir(&point).unwrap();
    let user = std::env::var("USER").expect("the user's name");
    let mut mount = Command::new(&sshfs_program);
    mount.args(["-F", "/dev/null", "-p", &sshd.port.to_string()]);
    for option in &sshd.options {
        mount.arg("-o").arg(option);
    }
    let mounted = mount
        .arg(format!("{user}@127.0.0.1:{}", served.display()))
        .arg(&point)
        .status()
        .expect("run sshfs");
    assert!(mounted.success());
    let left = Unmounted(point.clone());
    skwir_core::fs::reach::changed();
    assert!(skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point));

    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..Sshfs::default()
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let entries = small_files(&base.join("local"));
    let into = point.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(
        sshfs.rsync().made(),
        2,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_tree(
            e,
            &served.join("it's a folder").join(e.file_name().unwrap()),
        );
    }
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(sshfs.rsync().made(), 4);
    for e in &entries {
        same_tree(e, &down.join(e.file_name().unwrap()));
    }
    drop(app);
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// An SFTP-only account (`ForceCommand internal-sftp`) runs no rsync: rsync
/// finds so, makes nothing and leaves the batch to sftp, eight sessions on
/// the connection sshfs holds, which keep names, modes and times, and
/// place nothing over what is there. Down, Skwir's own copy goes through
/// the mount.
#[test]
fn an_sftp_only_server_copies_through_sftp_sessions() {
    let sshfs = Sshfs {
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        ..Sshfs::default()
    };
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    if skwir_core::os_path::find_program("sftp").is_none() {
        eprintln!("skipped: sftp is not installed");
        return;
    }
    let base = fixture("sftp-only");
    let Some(sshd) = OwnSshd::start_with(&base, "ForceCommand internal-sftp\n") else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    std::fs::create_dir_all(&served).unwrap();
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());

    let entries = small_files(&base.join("local"));
    let into = mounted.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    // A name taken there already: its file is left as it is.
    let taken = entries[1].file_name().unwrap().to_owned();
    std::fs::write(into.join(&taken), "the server's own").unwrap();
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(
        up.error.is_some_and(|e| e.contains("already there")),
        "the taken name stops the plan"
    );
    assert_eq!(sshfs.rsync().made(), 0, "no rsync to run");
    assert_eq!(
        sshfs.sftp().made(),
        2,
        "the folder staged, the files placed"
    );
    assert_eq!(
        std::fs::read_to_string(served.join("it's a folder").join(&taken)).unwrap(),
        "the server's own"
    );
    // Again without it: every copy made, by sftp.
    std::fs::remove_file(into.join(&taken)).unwrap();
    let names: Vec<_> = std::fs::read_dir(&into)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    for made in names {
        if made.is_dir() {
            std::fs::remove_dir_all(&made).unwrap();
        } else {
            std::fs::remove_file(&made).unwrap();
        }
    }
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(sshfs.sftp().made(), 4);
    for e in &entries {
        same_tree(
            e,
            &served.join("it's a folder").join(e.file_name().unwrap()),
        );
    }
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(
        sshfs.sftp().made(),
        4,
        "down, Skwir's own copy through the mount"
    );
    for e in &entries {
        same_tree(e, &down.join(e.file_name().unwrap()));
    }
    // Moved down: the originals removed together by sftp sessions, a tree
    // renamed aside first, as the pane and sshfs show at once.
    app.navigate(pane, &Location::from_path(&into)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    assert_eq!(names_shown(&app, pane), names_of(&entries));
    let moved_down = base.join("moved down");
    std::fs::create_dir(&moved_down).unwrap();
    let moved = app
        .executor
        .execute_blocking(&moves(&there, &moved_down), app.lifetime());
    assert!(moved.error.is_none(), "{:?}", moved.error);
    assert_eq!(sshfs.sftp().removed(), 1, "the originals removed by sftp");
    assert_eq!(sshfs.rsync().removed(), 0, "no rsync to run");
    assert_eq!(
        std::fs::read_dir(served.join("it's a folder"))
            .unwrap()
            .count(),
        0
    );
    for e in &entries {
        same_tree(e, &moved_down.join(e.file_name().unwrap()));
    }
    until_shown(&app, pane, 10, &[], "the pane shows the originals gone");
    std::fs::File::create_new(into.join("it's 00.txt")).unwrap();
    std::fs::create_dir(into.join("a folder")).unwrap();
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// GVfs's WebDAV keeps no modes nor times: a folder copied through its
/// bridge by Skwir's own copy is copied all the same (a mode the volume
/// keeps none of is no failure), and by eight `gio`, which serve WebDAV
/// too.
#[test]
fn gvfs_webdav_takes_copies_by_gio_and_through_its_bridge() {
    let Some(package) = std::env::var_os("SKWIR_TEST_GVFS").map(PathBuf::from) else {
        eprintln!("skipped: SKWIR_TEST_GVFS names no GVfs package");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("gvfs-dav");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let private = PrivateGvfs::start(&package, &base);
    let gvfs = skwir_plugin_remote::gvfs::Gvfs {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Default::default()
    };
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let address = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let connected = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let entries = small_files(&base.join("local"));
    let same_bytes = |src: &Path, copy: &Path| {
        fn walk(src: &Path, copy: &Path) {
            if src.is_dir() {
                for entry in std::fs::read_dir(src).unwrap() {
                    let name = entry.unwrap().file_name();
                    walk(&src.join(&name), &copy.join(&name));
                }
            } else {
                assert_eq!(
                    std::fs::read(src).unwrap(),
                    std::fs::read(copy).unwrap(),
                    "{}",
                    copy.display()
                );
            }
        }
        walk(src, copy);
    };
    for (folder, by_gio) in [("own", false), ("by gio", true)] {
        if by_gio {
            app.transfers.register("test", gvfs.gio_copy());
        }
        let into = connected.point.join(folder);
        std::fs::create_dir(&into).unwrap();
        let r = app
            .executor
            .execute_blocking(&copies(&entries, &into), app.lifetime());
        assert!(r.error.is_none(), "{folder}: {:?}", r.error);
        for e in &entries {
            same_bytes(e, &served_root.join(folder).join(e.file_name().unwrap()));
        }
    }
    assert_eq!(
        gvfs.gio_copy().made(),
        2,
        "the folder staged, the files placed"
    );
    drop(app);
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// `copy` holds the bytes `src` does, all the way down: what a server
/// that keeps neither modes nor times (AFP through GVfs) keeps.
fn same_bytes(src: &Path, copy: &Path) {
    if src.is_dir() {
        for entry in std::fs::read_dir(src).unwrap() {
            let name = entry.unwrap().file_name();
            same_bytes(&src.join(&name), &copy.join(&name));
        }
    } else {
        assert_eq!(
            std::fs::read(src).unwrap(),
            std::fs::read(copy).unwrap(),
            "{}",
            copy.display()
        );
    }
}

/// What a GVfs mount's kind takes by `gio` (`gio_copy::ways_of`) beside
/// batches up, within and removals: batches down, a file alone up and
/// down.
#[derive(Clone, Copy)]
struct ByGio {
    down: bool,
    alone_up: bool,
    alone_down: bool,
}

/// GVfs's NFS and AFP mounts take by `gio` what it does better
/// (2026-10-22): copies up, within a mount and removals, copies down from
/// NFS, and a file alone down from either; down from AFP its bridge,
/// sixteen files at once, was faster. Their servers (unfs3, netatalk) run as root:
/// only in namespaces of their own, which a script makes and mounts
/// through a GVfs of its own, naming the mounts' bridge folders in
/// `SKWIR_TEST_GVFS_NFS` and `SKWIR_TEST_GVFS_AFP`; its runtime folder
/// and bus are this process's (`XDG_RUNTIME_DIR`,
/// `DBUS_SESSION_BUS_ADDRESS`). Skipped elsewhere.
#[test]
fn gvfs_nfs_and_afp_take_by_gio_what_it_does_better() {
    let (Some(nfs), Some(afp)) = (
        std::env::var_os("SKWIR_TEST_GVFS_NFS"),
        std::env::var_os("SKWIR_TEST_GVFS_AFP"),
    ) else {
        eprintln!("skipped: no NFS and AFP mounted through GVfs by a script");
        return;
    };
    let nfs_ways = ByGio {
        down: true,
        alone_up: false,
        alone_down: true,
    };
    let afp_ways = ByGio {
        down: false,
        ..nfs_ways
    };
    takes_by_gio(
        "gvfs-nfs-afp",
        &[
            (PathBuf::from(nfs), nfs_ways),
            (PathBuf::from(afp), afp_ways),
        ],
    );
}

/// An NFS address below its export opens through Skwir's GVfs connector:
/// GVfs, given the address whole, finds the export that holds its path,
/// mounts it and says the mount is of that export, the folder asked below
/// it; Eject lets go of it. Against unfs3 in namespaces of its own, by
/// `tools/test/gvfs-nfs-afp.sh`, which names the export's folder in
/// `SKWIR_TEST_NFS_EXPORT` before anything is mounted; its runtime folder
/// and bus are this process's. Skipped elsewhere.
#[test]
fn gvfs_opens_an_nfs_address_below_its_export() {
    let Some(export) = std::env::var_os("SKWIR_TEST_NFS_EXPORT").map(PathBuf::from) else {
        eprintln!("skipped: no NFS export served in namespaces by a script");
        return;
    };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("the bridge's runtime folder");
    let gvfs = skwir_plugin_remote::gvfs::Gvfs {
        runtime: Some(PathBuf::from(runtime)),
        ..Default::default()
    };
    // The script's server outlives a run, its process numbers do not.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let below = format!("it's below {run}/é deeper");
    let folder = export.join(&below);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("note.txt"), "x").unwrap();
    let address = Address::parse(&format!("nfs://127.0.0.1{}/{below}/", export.display())).unwrap();
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let connected = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .unwrap_or_else(|error| panic!("{error:#}"));
    let left = Unmounted(connected.point.clone());
    assert_eq!(
        connected.address.path.strip_suffix(b"/"),
        Some(export.as_os_str().as_encoded_bytes()),
        "the export mounted: {}",
        connected.address
    );
    let inside = connected.address.below(&address).expect("the folder in it");
    assert_eq!(inside, format!("{below}/").as_bytes());
    let names: Vec<String> = std::fs::read_dir(
        connected
            .point
            .join(skwir_core::os_path::from_bytes(inside.to_vec())),
    )
    .unwrap()
    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
    .collect();
    assert_eq!(names, ["note.txt"]);

    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    assert_eq!(recognised.address.as_ref(), Some(&connected.address));
    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    // Gone from GVfs's bridge.
    let deadline = Instant::now() + Duration::from_secs(10);
    while connected.point.exists() {
        assert!(Instant::now() < deadline, "never let go of");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(left);
    std::fs::remove_dir_all(export.join(format!("it's below {run}"))).unwrap();
}

/// A phone's MTP takes by `gio` every way, a file alone too (2026-10-22,
/// a phone on USB 2). Only where a script mounted a phone through a GVfs
/// of its own and named a folder of its on the phone, which it removes
/// after, in `SKWIR_TEST_GVFS_MTP` (`just mtp-check`): never a phone
/// plugged in that no one offered. Skipped elsewhere.
#[test]
fn gvfs_mtp_takes_by_gio_every_way() {
    let Some(phone) = std::env::var_os("SKWIR_TEST_GVFS_MTP") else {
        eprintln!("skipped: no phone mounted over MTP through GVfs by a script");
        return;
    };
    let ways = ByGio {
        down: true,
        alone_up: true,
        alone_down: true,
    };
    takes_by_gio("gvfs-mtp", &[(PathBuf::from(phone), ways)]);
}

#[test]
fn gvfs_gphoto2_takes_by_gio_up_and_within() {
    let Some(camera) = std::env::var_os("SKWIR_TEST_GVFS_GPHOTO2") else {
        eprintln!("skipped: no camera mounted over gPhoto2 through GVfs by a script");
        return;
    };
    // Its bridge sends a file after its close: every copy up by `gio`.
    let ways = ByGio {
        down: true,
        alone_up: true,
        alone_down: false,
    };
    let camera = PathBuf::from(camera);
    takes_by_gio("gvfs-gphoto2", &[(camera.clone(), ways)]);
    // It moves nothing between its folders: without `gio` too, a copy is
    // written in place (ADR-083), Skwir's own batch and a file alone.
    let base = fixture("gvfs-gphoto2-own");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let entries = small_files(&base.join("local"));
    let into = camera.join(format!("own-{}", std::process::id()));
    std::fs::create_dir(&into).unwrap();
    let file = entries.iter().find(|e| e.is_file()).unwrap();
    let (batch, alone): (Vec<PathBuf>, Vec<PathBuf>) =
        entries.iter().cloned().partition(|e| e != file);
    for run in [batch, alone] {
        let r = app
            .executor
            .execute_blocking(&copies(&run, &into), app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
    }
    for e in &entries {
        same_bytes(e, &into.join(e.file_name().unwrap()));
    }
    let mut there: Vec<_> = std::fs::read_dir(&into)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    there.sort();
    let mut names: Vec<_> = entries
        .iter()
        .map(|e| e.file_name().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(there, names, "the copies alone, nothing staged beside them");
    for _ in 0..2 {
        let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
    }
    assert_eq!(std::fs::read_dir(&into).unwrap().count(), 0, "undone");
    std::fs::remove_dir(&into).unwrap();
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A phone GVfs's monitor offers (ADR-096), as the window has it: listed
/// among the devices by its volume's name before it is mounted, opened at
/// its address into its mount (the same place, now with Eject), let go of
/// by Eject (offered again), and said held by GVfs's camera program while
/// that holds it. Only where a script runs a GVfs of its own with a phone
/// plugged in and its USB port in `SKWIR_TEST_GVFS_PHONE` (`just
/// mtp-check`, which offers the phone): writes nothing to it. Skipped
/// elsewhere.
#[test]
fn gvfs_offers_a_phone_opens_it_and_lets_go_of_it() {
    let Some(port) = std::env::var_os("SKWIR_TEST_GVFS_PHONE") else {
        eprintln!("skipped: no phone offered by a script's GVfs");
        return;
    };
    let port = port.to_string_lossy().into_owned();
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("the bridge's runtime folder");
    let gvfs: Arc<dyn Connector> = Arc::new(skwir_plugin_remote::gvfs::Gvfs {
        runtime: Some(PathBuf::from(runtime)),
        ..Default::default()
    });
    let remotes = Remotes::with(vec![gvfs]);
    let base = fixture("gvfs-phone");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
    let phone = || {
        app.registry
            .places
            .all()
            .into_iter()
            .find(|p| p.group == "devices" && p.icon.as_deref() == Some("phone"))
    };

    // Offered, not mounted: at its address, by its volume's name.
    until(&app, 20, "the phone offered", || {
        phone().is_some_and(|p| &*p.location.scheme == "mtp")
    });
    let offered = phone().unwrap();
    let address = Address::of_location(&offered.location).unwrap();
    assert!(offered.unavailable.is_some() && offered.eject.is_none());
    assert_ne!(
        offered.title,
        address.label(),
        "named by its volume, not its serial"
    );
    assert_eq!(remotes.label_of(&address), offered.title);

    // Opened: its mount, the same place, with Eject.
    let point = remotes
        .reach(
            &app,
            &address,
            &(Arc::new(Answers::default()) as Arc<dyn Ask>),
            &CancelToken::new(),
        )
        .expect("the phone opens");
    until(&app, 20, "the phone shown mounted", || {
        phone().is_some_and(|p| p.location.path().as_deref() == Some(point.as_path()))
    });
    let mounted = phone().unwrap();
    assert_eq!(
        (mounted.title.as_str(), mounted.key.as_deref()),
        (offered.title.as_str(), offered.key.as_deref()),
        "the same place mounted or not"
    );
    assert!(mounted.eject.is_some() && mounted.unavailable.is_none());
    assert!(std::fs::read_dir(&point).is_ok(), "its storage listed");

    // Eject: let go of, offered again.
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "the phone let go of, offered again", || {
        phone().is_some_and(|p| &*p.location.scheme == "mtp") && !point.exists()
    });

    // Held as a camera by GVfs's gPhoto2 program: opening it says so.
    let camera = format!("gphoto2://[usb:{port}]/");
    let mounted_camera = (0..3).any(|_| {
        Command::new("gio")
            .args(["mount", &camera])
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    assert!(mounted_camera, "the phone mounted as a camera");
    let held = remotes
        .reach(
            &app,
            &address,
            &(Arc::new(Answers::default()) as Arc<dyn Ask>),
            &CancelToken::new(),
        )
        .map(|path| path.display().to_string());
    let _ = Command::new("gio")
        .args(["mount", "-u", &camera])
        .stdin(Stdio::null())
        .status();
    let error = held
        .expect_err("a phone held as a camera does not open")
        .to_string();
    assert!(
        error.starts_with(
            "gvfsd-gphoto2 has it open, and only one program at a time can use a phone"
        ),
        "{error}"
    );
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A phone KDE's kmtpd holds (ADR-097), as the window has it on Plasma:
/// offered by kmtpd's name for it, opened through Skwir's helper (the same
/// place mounted, with Eject), a folder and a file of the test's made,
/// read back and removed through it, let go of by Eject while kmtpd keeps
/// the phone. Only where a script runs a `kiod6` of its own on a bus of its
/// own with a phone plugged in and the helper's build in
/// `SKWIR_KMTP_WORKER` (`just mtp-check`, which offers the phone). Skipped
/// elsewhere.
#[test]
fn kmtpd_opens_a_phone_kde_holds_through_skwir_s_helper() {
    if std::env::var_os("SKWIR_TEST_KMTP_PHONE").is_none() {
        eprintln!("skipped: no phone held by a script's kiod6");
        return;
    }
    let kmtp: Arc<dyn Connector> = Arc::new(skwir_plugin_remote::kmtp::Kmtp {
        // KDE's desktop: kmtpd loaded as Dolphin would load it.
        on_kde: Some(true),
        ..Default::default()
    });
    let remotes = Remotes::with(vec![kmtp]);
    let base = fixture("kmtp-phone");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
    let phone = || {
        app.registry
            .places
            .all()
            .into_iter()
            .find(|p| p.group == "devices" && p.icon.as_deref() == Some("phone"))
    };
    until(&app, 60, "the phone offered by kmtpd", || {
        phone().is_some_and(|p| &*p.location.scheme == "mtp")
    });
    let offered = phone().unwrap();
    let address = Address::of_location(&offered.location).unwrap();
    assert_ne!(
        offered.title,
        address.label(),
        "named by kmtpd, not its serial"
    );

    let top = remotes
        .reach(
            &app,
            &address,
            &(Arc::new(Answers::default()) as Arc<dyn Ask>),
            &CancelToken::new(),
        )
        .expect("the phone opens through kmtpd");
    until(&app, 20, "the phone shown mounted", || {
        phone().is_some_and(|p| p.location.path().as_deref() == Some(top.as_path()))
    });
    assert_eq!(phone().unwrap().key, offered.key, "the same place");
    let storage = std::fs::read_dir(&top)
        .unwrap()
        .next()
        .expect("a storage")
        .unwrap()
        .path();
    assert!(
        std::fs::read_dir(&storage).unwrap().next().is_some(),
        "listed"
    );
    // A folder of the test's, a file in it, read back, removed.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let folder = storage.join(format!("skwir-test-{run}"));
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("it's written.txt");
    std::fs::write(&file, b"through kmtpd").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"through kmtpd");
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir(&folder).unwrap();
    assert!(!folder.exists());

    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": top }),
    ))
    .unwrap();
    until(&app, 30, "let go of, offered again", || {
        skwir_core::fs::reach::of(&top).is_none_or(|r| r.point != top)
            && phone().is_some_and(|p| &*p.location.scheme == "mtp")
    });
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// GVfs's AFC mount (an iPhone's) against a stand-in device on this
/// machine, by `tools/test/gvfs-afc.sh`: `gio` up, within, removed and for
/// a file alone down; down in batches through the bridge, which asks the
/// device fewer requests (ADR-078).
#[test]
fn gvfs_afc_takes_by_gio_what_it_does_better() {
    let Some(phone) = std::env::var_os("SKWIR_TEST_GVFS_AFC") else {
        eprintln!("skipped: no iPhone (AFC) mounted through GVfs by a script");
        return;
    };
    let ways = ByGio {
        down: false,
        alone_up: false,
        alone_down: true,
    };
    takes_by_gio("gvfs-afc", &[(PathBuf::from(phone), ways)]);
}

/// Copies up, down and within each of `mounts` (folders in GVfs's bridge,
/// whose runtime folder is this process's `XDG_RUNTIME_DIR`), files in
/// batches and a file alone, then removals: by `gio` where its ways say,
/// through the bridge otherwise, the same bytes either way.
fn takes_by_gio(name: &str, mounts: &[(PathBuf, ByGio)]) {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("the bridge's runtime folder");
    let gvfs = skwir_plugin_remote::gvfs::Gvfs {
        runtime: Some(PathBuf::from(runtime)),
        ..Default::default()
    };
    let gio = gvfs.gio_copy();
    let base = fixture(name);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.transfers.register("test", gio.clone());
    let entries = small_files(&base.join("local"));
    let file = entries.iter().find(|e| e.is_file()).unwrap().clone();
    let mut names: Vec<_> = entries.iter().map(|e| e.file_name().unwrap()).collect();
    names.sort();
    // The copies alone: no staging folder left beside them.
    let holds_the_copies = |folder: &Path| {
        let mut there: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        there.sort();
        assert_eq!(there, names, "{}", folder.display());
    };
    // The script's servers outlive a run, its process numbers do not.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    for (mount, ways) in mounts {
        let what = mount.display().to_string();
        let up = mount.join(format!("up-{run}"));
        std::fs::create_dir(&up).unwrap();
        let (made, removed) = (gio.made(), gio.removed());
        let r = app
            .executor
            .execute_blocking(&copies(&entries, &up), app.lifetime());
        assert!(r.error.is_none(), "{what} up: {:?}", r.error);
        // In batches: the folder staged, the files placed.
        assert!(gio.made() > made, "{what}: up by gio");
        holds_the_copies(&up);
        for e in &entries {
            same_bytes(e, &up.join(e.file_name().unwrap()));
        }

        let there: Vec<PathBuf> = entries
            .iter()
            .map(|e| up.join(e.file_name().unwrap()))
            .collect();
        let down = base.join(format!("down-{}", gio.made()));
        std::fs::create_dir(&down).unwrap();
        let before = gio.made();
        let r = app
            .executor
            .execute_blocking(&copies(&there, &down), app.lifetime());
        assert!(r.error.is_none(), "{what} down: {:?}", r.error);
        assert_eq!(gio.made() > before, ways.down, "{what}: down by gio or not");
        holds_the_copies(&down);
        for e in &entries {
            same_bytes(e, &down.join(e.file_name().unwrap()));
        }

        // A file alone, up and down.
        let one = mount.join(format!("one-{run}"));
        std::fs::create_dir(&one).unwrap();
        let before = gio.made();
        let r = app
            .executor
            .execute_blocking(&copies(std::slice::from_ref(&file), &one), app.lifetime());
        assert!(r.error.is_none(), "{what} one up: {:?}", r.error);
        assert_eq!(
            gio.made() > before,
            ways.alone_up,
            "{what}: one up by gio or not"
        );
        let sent = one.join(file.file_name().unwrap());
        same_bytes(&file, &sent);
        let one_down = base.join(format!("one-down-{}", gio.made()));
        std::fs::create_dir(&one_down).unwrap();
        let before = gio.made();
        let r = app.executor.execute_blocking(
            &copies(std::slice::from_ref(&sent), &one_down),
            app.lifetime(),
        );
        assert!(r.error.is_none(), "{what} one down: {:?}", r.error);
        assert_eq!(
            gio.made() > before,
            ways.alone_down,
            "{what}: one down by gio or not"
        );
        same_bytes(&file, &one_down.join(file.file_name().unwrap()));
        std::fs::remove_file(&sent).unwrap();
        let left: Vec<_> = std::fs::read_dir(&one)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(left.is_empty(), "{what}: left beside the copy: {left:?}");
        std::fs::remove_dir(&one).unwrap();

        let within = mount.join(format!("within-{run}"));
        std::fs::create_dir(&within).unwrap();
        let before = gio.made();
        let r = app
            .executor
            .execute_blocking(&copies(&there, &within), app.lifetime());
        assert!(r.error.is_none(), "{what} within: {:?}", r.error);
        assert!(gio.made() > before, "{what}: within by gio");
        holds_the_copies(&within);
        for e in &entries {
            same_bytes(e, &within.join(e.file_name().unwrap()));
        }

        for folder in [&up, &within] {
            let gone = skwir_core::op::Plan::with_steps(
                "delete",
                std::fs::read_dir(folder)
                    .unwrap()
                    .map(|e| {
                        Box::new(skwir_core::op::Delete {
                            entry: skwir_core::op::EntryRef::from_path(e.unwrap().path()).unwrap(),
                        }) as Box<dyn skwir_core::op::Operation>
                    })
                    .collect(),
            );
            let r = app.executor.execute_blocking(&gone, app.lifetime());
            assert!(r.error.is_none(), "{what} removed: {:?}", r.error);
            assert_eq!(std::fs::read_dir(folder).unwrap().count(), 0);
            std::fs::remove_dir(folder).unwrap();
        }
        assert_eq!(gio.removed(), removed + 2, "{what}: removed by gio");
    }
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A GVfs of the test's own: a session bus of its own, from which GVfs's
/// daemon starts, its FUSE bridge in a runtime folder of the test's, its
/// ssh's home the test's. `SKWIR_TEST_GVFS` names GVfs's package (its
/// `libexec`, `share` and `lib/gio/modules`); without it the test is
/// skipped, and the session's GVfs is never asked.
struct PrivateGvfs {
    bus: Child,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    runtime: PathBuf,
    /// Short, as a real one is (`/run/user/1000`): ssh's control socket
    /// GVfs makes in it must fit a socket's path (108 bytes). Sockets and
    /// a mount point only, nothing that needs the home's file system.
    _runtime: tempfile::TempDir,
}

impl PrivateGvfs {
    fn start(package: &Path, base: &Path) -> PrivateGvfs {
        use std::io::BufRead;
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let held = tempfile::Builder::new().prefix("g").tempdir().unwrap();
        let runtime = held.path().to_path_buf();
        std::fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
            ("HOME".into(), home.into()),
            ("XDG_RUNTIME_DIR".into(), runtime.clone().into()),
            ("XDG_DATA_DIRS".into(), package.join("share").into()),
            (
                "GIO_EXTRA_MODULES".into(),
                package.join("lib/gio/modules").into(),
            ),
            ("GIO_USE_VOLUME_MONITOR".into(), "unix".into()),
        ];
        // A bus that starts GVfs's services alone: nothing of the user's
        // session (a keyring, the portals) is started on it.
        let services = package.join("share/dbus-1/services");
        let config = base.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <servicedir>{}</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                runtime.display(),
                services.display()
            ),
        )
        .unwrap();
        let mut bus = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(base.join("bus.log")).unwrap())
            .spawn()
            .expect("start a session bus");
        let mut address = String::new();
        std::io::BufReader::new(bus.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        env.push((
            "DBUS_SESSION_BUS_ADDRESS".into(),
            address.trim().to_owned().into(),
        ));
        PrivateGvfs {
            bus,
            env,
            runtime,
            _runtime: held,
        }
    }
}

impl Drop for PrivateGvfs {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
        // Its daemon goes with its bus; its bridge, if it lingers, too.
        let _ = Command::new(fusermount())
            .arg("-uz")
            .arg(self.runtime.join("gvfs"))
            .stderr(Stdio::null())
            .status();
    }
}

#[test]
fn gvfs_mounts_through_gio_its_questions_answered_by_the_window() {
    let Some(package) = std::env::var_os("SKWIR_TEST_GVFS").map(PathBuf::from) else {
        eprintln!("skipped: SKWIR_TEST_GVFS names no GVfs package");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("gvfs");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("through gvfs.txt"), "hi").unwrap();
    let served = Served::start(
        "sftp",
        &served_root,
        &["--user", "tester", "--pass", "a test password"],
    );
    let private = PrivateGvfs::start(&package, &base);
    let gvfs = skwir_plugin_remote::gvfs::Gvfs {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Default::default()
    };
    let answers = Arc::new(Answers {
        password: "a test password".to_owned(),
        remember: true,
        ..Answers::default()
    });
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", served.port)).unwrap();
    // `gio mount`'s password asked through the keyring's layer (ADR-082).
    let keyring = Arc::new(TestKeyring::default());
    let keyed = Arc::new(Keyed::new(
        answers.clone(),
        keyring.clone(),
        address.top(),
        CancelToken::new(),
    ));
    let ask: Arc<dyn Ask> = keyed.clone();
    // Nothing started GVfs on the test's bus yet, as on a desktop other
    // than GNOME's: it can be connected through all the same, `gio mount`
    // starting its daemon, which starts its bridge.
    assert_eq!(gvfs.bridge(), None, "no GVfs runs on the test's bus yet");
    assert_eq!(gvfs.unavailable(), None, "GVfs is there to be started");
    let connected = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let asked = answers.asked.lock().unwrap().clone();
    assert!(
        asked.iter().any(|q| q.starts_with("password:")),
        "its password was asked for, as a server's: {asked:?}"
    );
    assert!(
        connected.point.starts_with(private.runtime.join("gvfs")),
        "{}",
        connected.point.display()
    );
    let names: Vec<String> = std::fs::read_dir(&connected.point)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"through gvfs.txt".to_owned()), "{names:?}");
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    assert_eq!(
        remote.point, connected.point,
        "a GVfs mount is a remote of its own"
    );
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    assert_eq!(recognised.address, Some(address.top()));
    assert_eq!(
        recognised.label,
        format!("tester@127.0.0.1:{}", served.port)
    );

    // Copies to and from it in batches by `gio` (ADR-078): the folder
    // staged, the files placed beside; up, then down.
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.transfers.register("test", gvfs.gio_copy());
    let entries = small_files(&base.join("local"));
    let into = connected.point.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&into)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(up.applied, entries.len());
    assert_eq!(
        gvfs.gio_copy().made(),
        2,
        "the folder staged, the files placed"
    );
    // Copied by gio, beside the bridge, which no watch sees: the pane
    // reads the folder again.
    until_shown(
        &app,
        pane,
        10,
        &names_of(&entries),
        "the pane shows what gio copied",
    );
    for e in &entries {
        same_files(
            e,
            &served_root
                .join("it's a folder")
                .join(e.file_name().unwrap()),
        );
    }
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(
        gvfs.gio_copy().made(),
        4,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_files(e, &down.join(e.file_name().unwrap()));
    }
    let names: Vec<String> = std::fs::read_dir(served_root.join("it's a folder"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), entries.len(), "nothing staged left: {names:?}");

    // Within the mount: by `gio` too, its daemon copying there.
    let within = connected.point.join("within");
    std::fs::create_dir(&within).unwrap();
    let inside = app
        .executor
        .execute_blocking(&copies(&there, &within), app.lifetime());
    assert!(inside.error.is_none(), "{:?}", inside.error);
    assert_eq!(gvfs.gio_copy().made(), 6, "within, by gio");
    for e in &entries {
        same_files(e, &served_root.join("within").join(e.file_name().unwrap()));
    }

    // Moved down: copied as copies are, the originals removed together at
    // the plan's end by `gio remove`, which the bridge sees at once.
    let moved = base.join("moved down");
    std::fs::create_dir(&moved).unwrap();
    let plan = skwir_core::op::Plan::with_steps(
        "move",
        there
            .iter()
            .map(|e| {
                Box::new(skwir_core::op::Move::new(
                    skwir_core::op::EntryRef::from_path(e).unwrap(),
                    moved.clone(),
                )) as Box<dyn skwir_core::op::Operation>
            })
            .collect(),
    );
    let gone = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(gone.error.is_none(), "{:?}", gone.error);
    assert_eq!(
        gvfs.gio_copy().removed(),
        1,
        "the originals removed together"
    );
    assert_eq!(
        std::fs::read_dir(&into).unwrap().count(),
        0,
        "gone, as the bridge says"
    );
    assert_eq!(
        std::fs::read_dir(served_root.join("it's a folder"))
            .unwrap()
            .count(),
        0
    );
    for e in &entries {
        same_files(e, &moved.join(e.file_name().unwrap()));
    }
    until_shown(&app, pane, 10, &[], "the pane shows the originals gone");
    drop(app);

    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    // Kept once it served; mounted again, the keyring answers `gio mount`.
    assert_eq!(
        keyed.served(),
        Some(Noted::Kept(format!(
            "sftp://tester@127.0.0.1:{}",
            served.port
        )))
    );
    let quiet = Arc::new(Answers::default());
    let ask: Arc<dyn Ask> = Arc::new(Keyed::new(
        quiet.clone(),
        keyring,
        address.top(),
        CancelToken::new(),
    ));
    let again = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect with the kept password");
    assert!(
        !quiet
            .asked
            .lock()
            .unwrap()
            .iter()
            .any(|q| q.starts_with("password:")),
        "the keyring answered"
    );
    let remote = skwir_core::fs::reach::of(&again.point).unwrap();
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// KIO of the test's own (ADR-081): a session bus of its own, from which
/// kio-fuse and KIO's daemon start, kio-fuse mounting in a runtime folder
/// of the test's, Qt drawing nothing. `SKWIR_TEST_KIO` names the packages
/// (kio-fuse's, KIO's: their `share` and Qt plugins), `:` between them;
/// without it the test is skipped, and the session's KIO is never asked.
struct PrivateKio {
    bus: Child,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    runtime: PathBuf,
    /// Short, as a real one is: KIO's sockets live in it (108 bytes).
    _runtime: tempfile::TempDir,
}

/// The KIO packages `SKWIR_TEST_KIO` names, if it does.
fn kio_packages() -> Option<Vec<PathBuf>> {
    let named = std::env::var_os("SKWIR_TEST_KIO")?;
    Some(std::env::split_paths(&named).collect())
}

impl PrivateKio {
    fn start(packages: &[PathBuf], base: &Path) -> PrivateKio {
        use std::fmt::Write as _;
        use std::io::BufRead;
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let held = tempfile::Builder::new().prefix("k").tempdir().unwrap();
        let runtime = held.path().to_path_buf();
        std::fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let joined =
            |below: &str| std::env::join_paths(packages.iter().map(|p| p.join(below))).unwrap();
        let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
            ("HOME".into(), home.into()),
            ("XDG_RUNTIME_DIR".into(), runtime.clone().into()),
            ("XDG_DATA_DIRS".into(), joined("share")),
            ("QT_PLUGIN_PATH".into(), joined("lib/qt-6/plugins")),
            ("QT_QPA_PLATFORM".into(), "offscreen".into()),
            // KIO's words and kio-fuse's (`strerror`) in English.
            ("LC_MESSAGES".into(), "C".into()),
        ];
        // kio-fuse's own account of what it is asked, timed, in the bus's
        // log, when asked for (`SKWIR_TEST_KIO_DEBUG`).
        if std::env::var_os("SKWIR_TEST_KIO_DEBUG").is_some() {
            env.extend([
                ("QT_FORCE_STDERR_LOGGING".into(), "1".into()),
                (
                    "QT_LOGGING_RULES".into(),
                    "org.kde.kio.fuse.debug=true".into(),
                ),
                (
                    "QT_MESSAGE_PATTERN".into(),
                    "%{time process} %{message}".into(),
                ),
            ]);
        }
        // A bus that starts KIO's services alone: nothing of the user's
        // session (KWallet, the portals) is started on it.
        let mut services = String::new();
        for dir in packages.iter().map(|p| p.join("share/dbus-1/services")) {
            if dir.is_dir() {
                let _ = writeln!(services, "  <servicedir>{}</servicedir>", dir.display());
            }
        }
        let config = base.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
{services}  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                runtime.display(),
            ),
        )
        .unwrap();
        let mut bus = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(base.join("bus.log")).unwrap())
            .spawn()
            .expect("start a session bus");
        let mut address = String::new();
        std::io::BufReader::new(bus.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        env.push((
            "DBUS_SESSION_BUS_ADDRESS".into(),
            address.trim().to_owned().into(),
        ));
        PrivateKio {
            bus,
            env,
            runtime,
            _runtime: held,
        }
    }
}

impl Drop for PrivateKio {
    fn drop(&mut self) {
        // kio-fuse's mount let go of first (lazily: nothing waits on it),
        // then what the bus started, by its pids, then the bus.
        for root in std::fs::read_dir(&self.runtime)
            .into_iter()
            .flatten()
            .flatten()
        {
            if root.file_name().to_string_lossy().starts_with("kio-fuse-") {
                let _ = Command::new(fusermount())
                    .arg("-uz")
                    .arg(root.path())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        // What the bus started runs apart from it (its helper ends, and
        // what it started is the session manager's then): known by the
        // runtime folder it was given, the test's own.
        let started = with_runtime(&self.runtime);
        signal("-TERM", &started);
        let _ = self.bus.kill();
        let _ = self.bus.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while started
            .iter()
            .any(|pid| Path::new(&format!("/proc/{pid}")).exists())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        signal("-KILL", &started);
    }
}

/// This user's processes started with `runtime` as their runtime folder.
fn with_runtime(runtime: &Path) -> Vec<u32> {
    let mut wanted = b"XDG_RUNTIME_DIR=".to_vec();
    wanted.extend_from_slice(runtime.as_os_str().as_encoded_bytes());
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != std::process::id())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|environ| environ.split(|b| *b == 0).any(|v| v == wanted))
        })
        .collect()
}

/// Put `body` at `path` (escaped) on the WebDAV server on `port`, as
/// another of its clients would: rclone keeps what it listed (its own
/// cache), and a file written in its folder behind it is not its until
/// then (with no cache, its cleanups ran into each other and it stopped
/// answering).
fn put_over_webdav(port: u16, path: &str, body: &[u8]) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "PUT {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    assert!(
        answer.starts_with("HTTP/1.1 201") || answer.starts_with("HTTP/1.1 204"),
        "{answer}"
    );
}

/// The names in `folder`, sorted.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn kio_fuse_mounts_each_server_a_folder_of_its_own_read_again_when_told() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("kio");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("sub folder")).unwrap();
    std::fs::write(served_root.join("sub folder/hello.txt"), "hello").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let private = PrivateKio::start(&packages, &base);
    let kio = Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    };
    // Nothing started kio-fuse on the test's bus yet: it is there to be.
    assert_eq!(kio.unavailable(), None, "kio-fuse is there to be started");
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let address = Address::parse(&format!("dav://127.0.0.1:{}/sub folder", served.port)).unwrap();
    let connected = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    assert!(
        connected.point.starts_with(&private.runtime)
            && connected
                .point
                .ends_with(format!("webdav/127.0.0.1:{}", served.port)),
        "{}",
        connected.point.display()
    );
    assert_eq!(
        connected.address,
        Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap(),
        "a server's folder holds all of it"
    );
    let folder = connected.point.join("sub folder");
    assert_eq!(names_in(&folder), ["hello.txt"]);
    let remote = skwir_core::fs::reach::of(&folder).unwrap();
    assert_eq!(
        remote.point, connected.point,
        "a server is a remote of its own"
    );
    let recognised = kio.recognises(&remote).expect("KIO's");
    assert_eq!(recognised.address, Some(connected.address.clone()));
    assert!(kio.reads_whole(&remote), "kio-fuse reads WebDAV whole");

    // What the server gains is not listed while kio-fuse keeps what it
    // listed (30 seconds), until it is told to forget.
    put_over_webdav(served.port, "/sub%20folder/later.txt", b"later");
    assert_eq!(names_in(&folder), ["hello.txt"], "what kio-fuse listed");
    (kio.forget(&remote).expect("told").0)();
    assert_eq!(names_in(&folder), ["hello.txt", "later.txt"]);

    // Copies through the mount, up and down: the same bytes. Over KIO's
    // WebDAV a file keeps no time and no mode (setting them is refused,
    // which a copy takes as a volume keeping none).
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let entries = small_files(&base.join("local"));
    let into = connected.point.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    for e in &entries {
        same_bytes(
            e,
            &served_root
                .join("it's a folder")
                .join(e.file_name().unwrap()),
        );
    }
    let kept = |path: &Path| std::fs::metadata(path).unwrap().modified().unwrap();
    assert_ne!(
        kept(&entries[0]),
        kept(
            &served_root
                .join("it's a folder")
                .join(entries[0].file_name().unwrap())
        ),
        "KIO's WebDAV keeps no time: once it does, say so in ADR-081"
    );
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    for e in &entries {
        same_bytes(e, &down.join(e.file_name().unwrap()));
    }
    drop(app);

    // An FTP server beside it: a folder of its own.
    let ftp_root = base.join("ftp");
    std::fs::create_dir_all(&ftp_root).unwrap();
    std::fs::write(ftp_root.join("over ftp.txt"), "ftp").unwrap();
    let ftp = Served::start("ftp", &ftp_root, &[]);
    // First, a folder it has not: kio-fuse fails, KDE is let ask (nothing
    // to ask here, so it says nothing), and kio-fuse's reason is told.
    // (Once a server is mounted, kio-fuse gives any path on it unlooked:
    // the folder's listing fails.)
    if skwir_core::os_path::find_program("kioclient").is_some() {
        let missing = Address::parse(&format!("ftp://127.0.0.1:{}/not there", ftp.port)).unwrap();
        let said = kio
            .connect(&missing, &ask, &CancelToken::new())
            .map(|_| ())
            .expect_err("no such folder");
        assert_eq!(
            said.to_string(),
            "KIO could not open it: No such file or directory"
        );
    }
    let address = Address::parse(&format!("ftp://127.0.0.1:{}/", ftp.port)).unwrap();
    let by_ftp = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect over FTP");
    assert!(
        by_ftp
            .point
            .ends_with(format!("ftp/127.0.0.1:{}", ftp.port)),
        "{}",
        by_ftp.point.display()
    );
    assert_eq!(names_in(&by_ftp.point), ["over ftp.txt"]);
    assert_ne!(
        skwir_core::fs::reach::of(&by_ftp.point).unwrap().lane(),
        remote.lane(),
        "a server that stops holds its own"
    );
    assert!(
        kio.disconnect(&remote, &recognised, &CancelToken::new())
            .is_err(),
        "kio-fuse lets go of no server"
    );
    drop(private);
    drop(ftp);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn kio_folders_open_from_their_address_and_f5_reads_the_server_again() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("kio-window");
    let served_root = base.join("served");
    for (folder, file) in [("sub folder", "hello.txt"), ("other", "there.txt")] {
        std::fs::create_dir_all(served_root.join(folder)).unwrap();
        std::fs::write(served_root.join(folder).join(file), file).unwrap();
    }
    let served = Served::start("webdav", &served_root, &[]);
    let private = PrivateKio::start(&packages, &base);
    let kio = Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    };
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    // KIO alone: never the session's.
    let remotes = Remotes::with(vec![Arc::new(kio)]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let open = |folder: &str, expected: &[&str]| {
        let address = format!("dav://127.0.0.1:{}/{folder}", served.port);
        app.navigate(pane, &Location::parse(&address)).unwrap();
        until(&app, 60, "the address led to its folder", || {
            app.pane_location(pane)
                .and_then(|l| l.path())
                .is_some_and(|p| p.ends_with(folder))
        });
        let expected: Vec<String> = expected.iter().map(|&n| n.to_owned()).collect();
        until_shown(&app, pane, 20, &expected, "the folder is listed");
        app.pane_location(pane).unwrap().path().unwrap()
    };
    let folder = open("sub folder", &["hello.txt"]);
    // Another folder of the server kio-fuse holds, not asked of it yet.
    let other = open("other", &["there.txt"]);
    assert_eq!(
        skwir_core::fs::reach::of(&other).unwrap().point,
        skwir_core::fs::reach::of(&folder).unwrap().point,
        "one server, one folder of kio-fuse's"
    );
    // F5 reads the server again: kio-fuse is told to forget first.
    put_over_webdav(served.port, "/other/later.txt", b"later");
    app.dispatch(&Invocation::new("refresh")).unwrap();
    until_shown(
        &app,
        pane,
        10,
        &["later.txt".to_owned(), "there.txt".to_owned()],
        "F5 shows what the server gained",
    );
    // Near, but read whole by kio-fuse: small files' thumbnails alone.
    until(&app, 20, "the remote is known", || {
        app.remote(&other).is_some_and(|(_, known)| known.is_some())
    });
    let (_, known) = app.remote(&other).unwrap();
    assert_eq!(
        known.unwrap().thumbnails,
        skwir_core::fs::reach::Thumbnails::Small
    );
    let place = app
        .registry
        .places
        .find(|p| p.location.path().is_some_and(|p| other.starts_with(&p)))
        .expect("the server in the sidebar");
    assert!(place.eject.is_none(), "kio-fuse lets go of no server");
    drop(app);
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_copy_its_server_refuses_as_it_is_closed_is_told_so_a_move_keeps_its_original() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("kio-refused");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("into")).unwrap();
    // A server that makes a file but takes no more than a MiB in all:
    // over WebDAV, kio-fuse makes the file as it is opened and sends it up
    // as it is closed, where the server refuses it. It reads its folders
    // again after 15 seconds (5 minutes by default).
    let served = Served::start(
        "webdav",
        &served_root,
        &[
            "--max-transfer",
            "1M",
            "--cutoff-mode",
            "hard",
            "--dir-cache-time",
            "15s",
        ],
    );
    let private = PrivateKio::start(&packages, &base);
    let kio = Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    };
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let address = Address::parse(&format!("dav://127.0.0.1:{}/into", served.port)).unwrap();
    let connected = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let into = connected.point.join("into");
    let local = base.join("local");
    std::fs::create_dir_all(&local).unwrap();
    let original = local.join("two MiB.bin");
    let bytes: Vec<u8> = (0..2u32 << 20).map(|i| (i % 251) as u8).collect();
    std::fs::write(&original, &bytes).unwrap();
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let copied = app.executor.execute_blocking(
        &copies(std::slice::from_ref(&original), &into),
        app.lifetime(),
    );
    // Told as its close said, never as a source gone.
    let said = copied.error.unwrap_or_default();
    assert!(
        said.contains("refused it as it was closed") && !said.contains("not there any more"),
        "its server refused it: the copy is not done ({said})"
    );
    assert!(
        !served_root.join("into/two MiB.bin").exists(),
        "nothing placed on the server"
    );
    let moved = app.executor.execute_blocking(
        &moves(std::slice::from_ref(&original), &into),
        app.lifetime(),
    );
    assert!(moved.error.is_some(), "nor the move's");
    assert_eq!(
        std::fs::read(&original).unwrap(),
        bytes,
        "the original is kept"
    );
    assert!(
        !served_root.join("into/two MiB.bin").exists(),
        "nothing placed on the server"
    );
    // What the refused copies left is swept once the server lets go of it:
    // rclone's keeps an entry of the upload it cut off (no folder, the
    // folder never empty) until it reads the folder again, and kio-fuse is
    // told to forget what it listed first (`App::sweep_left_on`, with the
    // forget the plugin knows the mount by).
    let remotes = Remotes::with(vec![Arc::new(Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    })]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    until(&app, 20, "the mount is known with its forget", || {
        app.remote(&into)
            .is_some_and(|(_, known)| known.is_some_and(|k| k.forget.is_some()))
    });
    let left = || {
        std::fs::read_dir(served_root.join("into"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".skwir-copy-"))
            .count()
    };
    assert!(left() > 0, "the refused copies left their folder");
    let deadline = Instant::now() + Duration::from_secs(60);
    while left() > 0 {
        assert!(
            Instant::now() < deadline,
            "what the refused copies left stays"
        );
        app.sweep_left_on(&connected.point);
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(app);
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The window's next password question, within ten seconds.
fn secret_asked(app: &App) -> skwir_core::app::SecretQuestion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        if let Some(question) = app.take_secret_questions().pop() {
            return question;
        }
        assert!(Instant::now() < deadline, "never asked");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Open `address` in `pane` and wait for its folder listed as `names`,
/// asked nothing (a question would hold it).
fn opened_unasked(app: &App, pane: skwir_core::workspace::PaneId, address: &str, names: &[&str]) {
    app.navigate(pane, &Location::parse(address)).unwrap();
    let expected: Vec<String> = names.iter().map(|&n| n.to_owned()).collect();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.pump();
        assert!(
            app.take_secret_questions().is_empty(),
            "nothing is asked: the keyring answers"
        );
        if app.pane_location(pane).is_some_and(|l| l.is_path())
            && names_shown(app, pane) == expected
        {
            return;
        }
        assert!(Instant::now() < deadline, "{address} never listed");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Let go of the remote showing in `pane`, and wait until it is.
fn disconnected(app: &App, pane: skwir_core::workspace::PaneId) {
    let folder = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&folder).unwrap().point.clone();
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
}

#[test]
fn rclone_asks_a_server_s_password_once_and_keeps_it_when_told() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone-password");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("locked.txt"), "hi").unwrap();
    let served = Served::start(
        "webdav",
        &served_root,
        &["--user", "tester", "--pass", "a test password"],
    );
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.set_window_shown();
    let keyring = Arc::new(TestKeyring::default());
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]).with_keyring(keyring.clone());
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let address = format!("dav://tester@127.0.0.1:{}/", served.port);
    let key = format!("dav://tester@127.0.0.1:{}", served.port);

    // rclone asks nothing itself: refused without one, the window is
    // asked, offering to remember (ADR-082); kept once it served.
    app.navigate(pane, &Location::parse(&address)).unwrap();
    let question = secret_asked(&app);
    assert!(question.remember, "a server's password may be kept");
    assert!(
        question.title.contains("tester@127.0.0.1"),
        "{}",
        question.title
    );
    app.answer_secret(
        question.id,
        Some(skwir_core::app::SecretAnswer {
            text: "a test password".to_owned(),
            remember: true,
        }),
    );
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    until_shown(&app, pane, 20, &["locked.txt".to_owned()], "listed");
    let folder = app.pane_location(pane).unwrap().path().unwrap();
    let _left = Unmounted(skwir_core::fs::reach::of(&folder).unwrap().point.clone());
    assert_eq!(
        keyring.own.lock().unwrap().get(&key).map(String::as_str),
        Some("a test password")
    );
    let remembered: Vec<String> = app.settings.get("remote.remembered").unwrap_or_default();
    assert_eq!(
        remembered,
        std::slice::from_ref(&key),
        "the server noted, never its password"
    );
    until(&app, 20, "its place offers Forget Password", || {
        app.registry
            .places
            .find(|p| {
                p.group == "network" && p.menu.iter().any(|item| item.title == "Forget Password")
            })
            .is_some()
    });

    // Again: the keyring answers, the window is asked nothing.
    disconnected(&app, pane);
    opened_unasked(&app, pane, &address, &["locked.txt"]);

    // Forget Password: gone from the keyring and from what is noted.
    app.dispatch(&Invocation::with_args(
        "remote.forget_password",
        &serde_json::json!({ "server": key }),
    ))
    .unwrap();
    until(&app, 10, "forgotten", || {
        keyring.own.lock().unwrap().is_empty()
            && app
                .settings
                .get::<Vec<String>>("remote.remembered")
                .unwrap_or_default()
                .is_empty()
    });

    // A password GVfs kept for it (Nautilus told to remember): given,
    // read only, nothing asked.
    keyring.desktop.lock().unwrap().push((
        vec![
            ("protocol".to_owned(), "http".to_owned()),
            ("server".to_owned(), "127.0.0.1".to_owned()),
            ("user".to_owned(), "tester".to_owned()),
            ("object".to_owned(), "a realm".to_owned()),
        ],
        "a test password".to_owned(),
    ));
    disconnected(&app, pane);
    opened_unasked(&app, pane, &address, &["locked.txt"]);
    assert!(
        keyring.own.lock().unwrap().is_empty(),
        "GVfs's is not copied"
    );
    disconnected(&app, pane);
    drop(app);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Samba's server of the test's own, run as the user: it believes it is
/// root (`uid_wrapper`), and its user `tester` is the user (`nss_wrapper`).
/// `share` asks `tester`'s password ("a test password"); `public` lets a
/// guest in. `SKWIR_TEST_SAMBA` names Samba's, `uid_wrapper`'s and
/// `nss_wrapper`'s packages, `:` between them (`just remote-check`); without
/// it the test is skipped. Stopped when dropped, its sessions with it.
struct OwnSamba {
    child: Child,
    port: u16,
    smbclient: PathBuf,
    config: PathBuf,
}

impl OwnSamba {
    fn start(base: &Path) -> Option<OwnSamba> {
        let packages: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("SKWIR_TEST_SAMBA")?).collect();
        let [samba, uid_wrapper, nss_wrapper] = packages.as_slice() else {
            panic!("SKWIR_TEST_SAMBA names Samba's, uid_wrapper's and nss_wrapper's packages");
        };
        for folder in [
            "share", "public", "private", "lock", "state", "cache", "pid", "ncalrpc", "log",
        ] {
            std::fs::create_dir_all(base.join(folder)).unwrap();
        }
        let (uid, gid) = {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(base).unwrap();
            (meta.uid(), meta.gid())
        };
        std::fs::write(
            base.join("passwd"),
            format!(
                "root:x:0:0:root:/root:/bin/sh\ntester:x:{uid}:{gid}:tester:{}:/bin/sh\n\
                 nobody:x:65534:65534:nobody:/:/bin/false\n",
                base.display()
            ),
        )
        .unwrap();
        std::fs::write(
            base.join("group"),
            format!("root:x:0:\ntester:x:{gid}:\nnogroup:x:65534:\n"),
        )
        .unwrap();
        let port = free_port();
        let dir = |name: &str| base.join(name).display().to_string();
        let config = base.join("smb.conf");
        std::fs::write(
            &config,
            format!(
                "[global]\n  netbios name = SKWIRTEST\n  workgroup = WORKGROUP\n\
                 server role = standalone server\n  smb ports = {port}\n\
                 interfaces = 127.0.0.1\n  bind interfaces only = yes\n\
                 private dir = {}\n  lock directory = {}\n  state directory = {}\n\
                 cache directory = {}\n  pid directory = {}\n  ncalrpc dir = {}\n\
                 log file = {}/smbd.log\n  passdb backend = tdbsam:{}/passdb.tdb\n\
                 load printers = no\n  disable spoolss = yes\n\
                 server min protocol = SMB2\n  map to guest = Bad User\n\
                 [share]\n  path = {}\n  read only = no\n  valid users = tester\n\
                 [public]\n  path = {}\n  read only = no\n  guest ok = yes\n\
                 force user = tester\n",
                dir("private"),
                dir("lock"),
                dir("state"),
                dir("cache"),
                dir("pid"),
                dir("ncalrpc"),
                dir("log"),
                dir("private"),
                dir("share"),
                dir("public"),
            ),
        )
        .unwrap();
        let wrapped = |program: &Path| {
            let mut command = Command::new(program);
            command
                .env(
                    "LD_PRELOAD",
                    format!(
                        "{}:{}",
                        uid_wrapper.join("lib/libuid_wrapper.so").display(),
                        nss_wrapper.join("lib/libnss_wrapper.so").display()
                    ),
                )
                .env("UID_WRAPPER", "1")
                .env("UID_WRAPPER_ROOT", "1")
                .env("NSS_WRAPPER_PASSWD", base.join("passwd"))
                .env("NSS_WRAPPER_GROUP", base.join("group"));
            command
        };
        let mut pdbedit = wrapped(&samba.join("bin/pdbedit"));
        pdbedit
            .arg("-s")
            .arg(&config)
            .args(["-a", "-u", "tester", "-t"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut adding = pdbedit.spawn().expect("run pdbedit");
        {
            use std::io::Write;
            adding
                .stdin
                .take()
                .unwrap()
                .write_all(b"a test password\na test password\n")
                .unwrap();
        }
        assert!(adding.wait().unwrap().success(), "tester added");
        let child = wrapped(&samba.join("bin/smbd"))
            .arg("-s")
            .arg(&config)
            .args(["--foreground", "--no-process-group", "--debug-stdout"])
            .stdout(std::fs::File::create(base.join("smbd.out")).unwrap())
            .stderr(Stdio::null())
            .spawn()
            .expect("start smbd");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "smbd never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(OwnSamba {
            child,
            port,
            smbclient: samba.join("bin/smbclient"),
            config,
        })
    }

    /// What `share` holds, listed by Samba's own client.
    fn listed(&self, share: &str) -> String {
        let out = Command::new(&self.smbclient)
            .arg("-s")
            .arg(&self.config)
            .args(["-p", &self.port.to_string()])
            .arg(format!("//127.0.0.1/{share}"))
            .args(["-U", "tester%a test password", "-c", "ls"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for OwnSamba {
    fn drop(&mut self) {
        let sessions = descendants(self.child.id());
        let _ = self.child.kill();
        signal("-9", &sessions);
        let _ = self.child.wait();
    }
}

#[test]
fn rclone_opens_an_smb_share_its_password_asked_and_kept() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("smb-rclone");
    let Some(samba) = OwnSamba::start(&base) else {
        eprintln!("skipped: SKWIR_TEST_SAMBA names no Samba");
        return;
    };
    std::fs::write(base.join("share/over smb.txt"), "hi").unwrap();
    assert!(
        samba.listed("share").contains("over smb.txt"),
        "Samba serves it"
    );
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.set_window_shown();
    let keyring = Arc::new(TestKeyring::default());
    let remotes = Remotes::with(vec![Arc::new(Rclone::default())]).with_keyring(keyring.clone());
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    let pane = app.workspace.read().active_id();
    let address = format!("smb://tester@127.0.0.1:{}/share", samba.port);
    app.navigate(pane, &Location::parse(&address)).unwrap();
    let question = secret_asked(&app);
    assert!(question.remember);
    app.answer_secret(
        question.id,
        Some(skwir_core::app::SecretAnswer {
            text: "a test password".to_owned(),
            remember: true,
        }),
    );
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    until_shown(&app, pane, 20, &["over smb.txt".to_owned()], "listed");
    let folder = app.pane_location(pane).unwrap().path().unwrap();
    let _left = Unmounted(skwir_core::fs::reach::of(&folder).unwrap().point.clone());
    let key = format!("smb://tester@127.0.0.1:{}", samba.port);
    assert_eq!(
        keyring.own.lock().unwrap().get(&key).map(String::as_str),
        Some("a test password")
    );
    // Written through the mount: on the server.
    std::fs::write(folder.join("from skwir.txt"), "written").unwrap();
    until(&app, 20, "on the server", || {
        std::fs::read_to_string(base.join("share/from skwir.txt")).is_ok_and(|t| t == "written")
    });
    disconnected(&app, pane);
    opened_unasked(&app, pane, &address, &["from skwir.txt", "over smb.txt"]);
    disconnected(&app, pane);
    drop(app);
    drop(samba);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn kio_opens_an_smb_share() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    let base = fixture("smb-kio");
    let Some(samba) = OwnSamba::start(&base) else {
        eprintln!("skipped: SKWIR_TEST_SAMBA names no Samba");
        return;
    };
    std::fs::write(base.join("public/over smb.txt"), "hi").unwrap();
    let private = PrivateKio::start(&packages, &base.join("kio"));
    let kio = Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    };
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    // A share a guest may open: KIO's password dialog no test answers.
    let address = Address::parse(&format!("smb://127.0.0.1:{}/public", samba.port)).unwrap();
    let connected = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect over SMB");
    assert!(
        connected
            .point
            .ends_with(format!("smb/127.0.0.1:{}", samba.port)),
        "{}",
        connected.point.display()
    );
    let folder = connected.point.join("public");
    assert_eq!(names_in(&folder), ["over smb.txt"]);
    let remote = skwir_core::fs::reach::of(&folder).unwrap();
    assert!(!kio.reads_whole(&remote), "SMB is read in place");
    // Copies through the mount, up and down: the same bytes.
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let entries = small_files(&base.join("local"));
    let into = folder.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    for e in &entries {
        same_bytes(
            e,
            &base
                .join("public/it's a folder")
                .join(e.file_name().unwrap()),
        );
    }
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    for e in &entries {
        same_bytes(e, &down.join(e.file_name().unwrap()));
    }
    drop(app);
    drop(private);
    drop(samba);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Each refusal `Gvfs::disconnect` asks again after, as its log says.
struct Refusals(std::sync::mpsc::Sender<()>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Refusals {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.fields().any(|field| field.name() == "refused") {
            let _ = self.0.send(());
        }
    }
}

#[test]
fn gvfs_opens_an_smb_share_its_password_kept() {
    let Some(package) = std::env::var_os("SKWIR_TEST_GVFS").map(PathBuf::from) else {
        eprintln!("skipped: SKWIR_TEST_GVFS names no GVfs package");
        return;
    };
    let base = fixture("smb-gvfs");
    let Some(samba) = OwnSamba::start(&base) else {
        eprintln!("skipped: SKWIR_TEST_SAMBA names no Samba");
        return;
    };
    std::fs::write(base.join("share/over smb.txt"), "hi").unwrap();
    let private = PrivateGvfs::start(&package, &base.join("gvfs"));
    let gvfs = skwir_plugin_remote::gvfs::Gvfs {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Default::default()
    };
    let address = Address::parse(&format!("smb://tester@127.0.0.1:{}/share", samba.port)).unwrap();
    let answers = Arc::new(Answers {
        password: "a test password".to_owned(),
        remember: true,
        ..Answers::default()
    });
    let keyring = Arc::new(TestKeyring::default());
    let keyed = Arc::new(Keyed::new(
        answers.clone(),
        keyring.clone(),
        address.top(),
        CancelToken::new(),
    ));
    let ask: Arc<dyn Ask> = keyed.clone();
    let connected = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect over SMB");
    assert!(
        answers
            .asked
            .lock()
            .unwrap()
            .iter()
            .any(|q| q.starts_with("password:")),
        "its password was asked for, as a server's"
    );
    assert_eq!(names_in(&connected.point), ["over smb.txt"]);
    assert_eq!(
        keyed.served(),
        Some(Noted::Kept(format!(
            "smb://tester@127.0.0.1:{}",
            samba.port
        )))
    );
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    // GVfs refuses to let go as busy while one of its reads runs (a read
    // through its bridge; under load, a folder just listed), not while a
    // file is merely open: asked again until it lets go. A read Samba
    // does not answer (its processes stopped) keeps it busy until the
    // test ends it, however long the machine takes to read.
    std::fs::write(base.join("share/large.bin"), vec![7u8; 8 << 20]).unwrap();
    let large = connected.point.join("large.bin");
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (refused, refusals) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let mut smbd = descendants(samba.child.id());
        smbd.push(samba.child.id());
        signal("-STOP", &smbd);
        // Let go however the test ends, before its threads are waited for.
        let mut thawed = Thawed(Some(smbd));
        let reader = scope.spawn(|| {
            use std::io::Read;
            let mut file = std::fs::File::open(&large)?;
            let mut chunk = vec![0u8; 1 << 20];
            let mut read = 0;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                match file.read(&mut chunk)? {
                    0 => break,
                    n => read += n,
                }
            }
            Ok::<_, std::io::Error>(read)
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while waiting_on_fuse().is_empty() {
            assert!(Instant::now() < deadline, "a read waits on Samba");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (gvfs, remote, recognised) = (&gvfs, &remote, &recognised);
        let disconnecting = scope.spawn(move || {
            use tracing_subscriber::layer::SubscriberExt;
            let logged = tracing_subscriber::registry().with(Refusals(refused));
            let _logged = tracing::subscriber::set_default(logged);
            gvfs.disconnect(remote, recognised, &CancelToken::new())
        });
        refusals
            .recv_timeout(Duration::from_secs(15))
            .expect("GVfs refused while its read waited, and was asked again");
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        signal("-CONT", &thawed.0.take().unwrap_or_default());
        disconnecting
            .join()
            .unwrap()
            .expect("disconnect, once GVfs let go");
        // The reader ends once its read is answered, or is cut where GVfs
        // let go between two of the reads that one asked.
        if let Err(error) = reader.join().unwrap() {
            assert!(
                matches!(error.raw_os_error(), Some(5 | 107)),
                "cut as GVfs let go: {error}"
            );
        }
    });
    // Again: the keyring answers `gio mount`.
    let quiet = Arc::new(Answers::default());
    let ask: Arc<dyn Ask> = Arc::new(Keyed::new(
        quiet.clone(),
        keyring,
        address.top(),
        CancelToken::new(),
    ));
    let again = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect with the kept password");
    assert!(
        !quiet
            .asked
            .lock()
            .unwrap()
            .iter()
            .any(|q| q.starts_with("password:")),
        "the keyring answered"
    );
    let remote = skwir_core::fs::reach::of(&again.point).unwrap();
    let recognised = gvfs.recognises(&remote).expect("GVfs's");
    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(private);
    drop(samba);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn kioclient_copies_to_and_from_kio_s_servers_in_batches() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    if skwir_core::os_path::find_program("kioclient").is_none() {
        eprintln!("skipped: kioclient (KDE) is not on PATH");
        return;
    }
    let base = fixture("kioclient");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let private = PrivateKio::start(&packages, &base.join("kio"));
    let kio = Kio {
        env: private.env.clone(),
        runtime: Some(private.runtime.clone()),
        ..Kio::default()
    };
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    let address = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let connected = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.transfers.register("test", kio.kio_copy());
    let entries = small_files(&base.join("local"));
    let into = connected.point.join("it's a folder");
    std::fs::create_dir(&into).unwrap();
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&into)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));

    // Up: the folder staged, the files placed beside, by `kioclient`.
    let up = app
        .executor
        .execute_blocking(&copies(&entries, &into), app.lifetime());
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(
        kio.kio_copy().made(),
        2,
        "the folder staged, the files placed"
    );
    for e in &entries {
        same_bytes(
            e,
            &served_root
                .join("it's a folder")
                .join(e.file_name().unwrap()),
        );
    }
    // Copied beside kio-fuse, which is told to forget: the pane shows them.
    until_shown(
        &app,
        pane,
        10,
        &names_of(&entries),
        "the pane shows what kioclient copied",
    );

    // Down, then moved down: the originals removed together by
    // `kioclient`, which kio-fuse sees at once.
    let there: Vec<PathBuf> = entries
        .iter()
        .map(|e| into.join(e.file_name().unwrap()))
        .collect();
    let down = base.join("down");
    std::fs::create_dir(&down).unwrap();
    let back = app
        .executor
        .execute_blocking(&copies(&there, &down), app.lifetime());
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(kio.kio_copy().made(), 4);
    for e in &entries {
        same_bytes(e, &down.join(e.file_name().unwrap()));
    }
    let moved = base.join("moved down");
    std::fs::create_dir(&moved).unwrap();
    let gone = app
        .executor
        .execute_blocking(&moves(&there, &moved), app.lifetime());
    assert!(gone.error.is_none(), "{:?}", gone.error);
    assert_eq!(
        kio.kio_copy().removed(),
        1,
        "the originals removed together"
    );
    assert_eq!(
        std::fs::read_dir(&into).unwrap().count(),
        0,
        "gone, as kio-fuse says"
    );
    for e in &entries {
        same_bytes(e, &moved.join(e.file_name().unwrap()));
    }
    until_shown(&app, pane, 10, &[], "the pane shows the originals gone");
    drop(app);

    // FTP: a file alone goes up by `kioclient` too, which kio-fuse cannot
    // write there (KIO's FTP worker reads back no empty file).
    let ftp_root = base.join("ftp");
    std::fs::create_dir_all(&ftp_root).unwrap();
    let ftp = Served::start("ftp", &ftp_root, &[]);
    let address = Address::parse(&format!("ftp://127.0.0.1:{}/", ftp.port)).unwrap();
    let by_ftp = kio
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect over FTP");
    let app = App::new(AppConfig::ephemeral(base.join("app2"))).unwrap();
    app.transfers.register("test", kio.kio_copy());
    let alone = base.join("alone.bin");
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&alone, &bytes).unwrap();
    let made = kio.kio_copy().made();
    let up = app.executor.execute_blocking(
        &copies(std::slice::from_ref(&alone), &by_ftp.point),
        app.lifetime(),
    );
    assert!(up.error.is_none(), "{:?}", up.error);
    assert_eq!(kio.kio_copy().made(), made + 1, "by kioclient");
    assert_eq!(std::fs::read(ftp_root.join("alone.bin")).unwrap(), bytes);
    drop(app);
    drop(private);
    drop(ftp);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The threads of this process waiting for a FUSE server's answer, by
/// what the system says each sleeps in.
fn waiting_on_fuse() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("wchan")).ok())
        .filter(|wchan| wchan.contains("fuse") || wchan.contains("request_wait_answer"))
        .collect()
}

/// A server's sessions stopped (`SIGSTOP`), let go of however a test
/// ends: they are not its server's children, and stopped they stay the
/// test's, so their numbers name no other process. `None` once thawed.
struct Thawed(Option<Vec<u32>>);

impl Drop for Thawed {
    fn drop(&mut self) {
        if let Some(sessions) = self.0.take() {
            signal("-CONT", &sessions);
            signal("-9", &sessions);
        }
    }
}

/// A server that stops answering holds no more of Skwir than its lane:
/// a search of its folder and a count of its tags walk it a folder at a
/// time on their jobs' threads, never threads of their own asking it all
/// at once, and an agent's call naming a file there is answered within
/// `reach::NOT_ANSWERING`, the file kept from it as private.
#[test]
fn a_server_that_stops_answering_holds_no_more_than_its_lane() {
    let sshfs = Sshfs::default();
    if let Some(why) = sshfs.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("held-lane");
    let Some(mut sshd) = OwnSshd::start(&base) else {
        std::fs::remove_dir_all(&base).unwrap();
        return;
    };
    let served = base.join("served");
    for n in 0..40 {
        let folder = served.join(format!("folder {n}")).join("it's inner");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("note {n}.txt")), "x").unwrap();
    }
    let sshfs = Arc::new(Sshfs {
        ssh_options: sshd.options.clone(),
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..sshfs
    });
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![sshfs.clone() as Arc<dyn Connector>]);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    let user = std::env::var("USER").expect("the user's name");
    let address = format!("sftp://{user}@127.0.0.1:{}{}", sshd.port, served.display());
    app.navigate(pane, &Location::parse(&address)).unwrap();
    until(&app, 60, "the address led to its folder", || {
        app.pane_location(pane).is_some_and(|l| l.is_path())
    });
    let mounted = app.pane_location(pane).unwrap().path().unwrap();
    let point = skwir_core::fs::reach::of(&mounted).unwrap().point.clone();
    let left = Unmounted(point.clone());
    until(&app, 30, "its folder was listed", || {
        app.workspace
            .read()
            .pane(pane)
            .is_some_and(|p| p.listing.is_complete())
    });
    // What the listing asked of it answered (under load, a read may
    // still be on its way).
    until(&app, 30, "nothing waits on it yet", || {
        waiting_on_fuse().is_empty()
    });

    let sessions = sshd.descendants();
    signal("-STOP", &sessions);
    // Let go however the test ends.
    let mut thawed = Thawed(Some(sessions));
    let search = app.workspace.write().add_pane();
    app.navigate(
        search,
        &skwir_plugin_query::location_for("ext:txt", Some(&mounted)),
    )
    .unwrap();
    let tags = app.workspace.write().add_pane();
    app.dispatch(&Invocation::with_args(
        "tags.browse",
        &serde_json::json!({ "in": mounted, "pane": tags }),
    ))
    .unwrap();
    // An agent's call naming a file there.
    let asked = Instant::now();
    let answer = skwir_plugin_control::run(
        &app,
        &Invocation::with_args(
            "query.search",
            &serde_json::json!({ "expr": "ext:txt", "root": mounted.join("folder 1"), "pane": search }),
        ),
        skwir_plugin_control::PlanMode::Propose,
        &|| false,
    );
    let waited = asked.elapsed();
    let error = answer.expect_err("a file whose privacy is unknown is private");
    assert!(error.to_string().contains("private"), "{error:#}");
    assert!(
        waited < skwir_core::fs::reach::NOT_ANSWERING + Duration::from_secs(3),
        "answered after {waited:?}"
    );
    std::thread::sleep(Duration::from_secs(2));
    let waiting = waiting_on_fuse();
    eprintln!("waiting on the server: {waiting:?}");
    assert!(
        waiting.len() <= skwir_core::scheduler::LANE_RUNS,
        "no more than its lane waits on it: {waiting:?}"
    );

    signal("-CONT", &thawed.0.take().unwrap_or_default());
    until(&app, 60, "the search ended", || {
        app.workspace
            .read()
            .pane(search)
            .is_some_and(|p| p.listing.is_complete())
    });
    until(&app, 60, "nothing waits on it any more", || {
        waiting_on_fuse().is_empty()
    });
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    until(&app, 30, "it was let go of", || {
        skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point)
    });
    drop(left);
    sshd.stop();
    drop(thawed);
    drop(sshd);
    // What the app writes as it ends goes before its folder does.
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A WebDAV share is where its server serves it, its server's root often
/// none (Nextcloud's `/remote.php/dav/files/…`; here rclone's
/// `--baseurl`, under which alone it answers): its address opens through
/// rclone and through GVfs, each mounting the share asked, its files
/// there, and saying it is of that share.
#[test]
fn a_webdav_share_below_its_server_s_root_opens_where_it_is() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("dav-share");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("it's a folder")).unwrap();
    std::fs::write(served_root.join("it's a folder/note é.txt"), "x").unwrap();
    let served = Served::start(
        "webdav",
        &served_root,
        &["--baseurl", "/remote.php/dav/files/me"],
    );
    let address = Address::parse(&format!(
        "dav://127.0.0.1:{}/remote.php/dav/files/me/it's a folder",
        served.port
    ))
    .unwrap();
    let share = Address::parse(&format!(
        "dav://127.0.0.1:{}/remote.php/dav/files/me/it's a folder/",
        served.port
    ))
    .unwrap();
    let ask: Arc<dyn Ask> = Arc::new(Answers::default());
    // What the connector mounted (rclone: the share asked; GVfs: the
    // collection its server serves, the share asked below it) holds the
    // share's files where the address says.
    let opened = |connector: &dyn Connector, mounted: &Address| {
        let connected = connector
            .connect(&address, &ask, &CancelToken::new())
            .unwrap_or_else(|error| panic!("{}: {error:#}", connector.id()));
        let left = Unmounted(connected.point.clone());
        assert_eq!(
            &connected.address,
            mounted,
            "{}: what it mounted",
            connector.id()
        );
        let below = connected.address.below(&address).expect("the share in it");
        let folder = connected
            .point
            .join(skwir_core::os_path::from_bytes(below.to_vec()));
        let names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["note é.txt"],
            "{}: the share's files",
            connector.id()
        );
        (connected, left)
    };

    let rclone = Rclone::default();
    let (connected, left) = opened(&rclone, &share);
    let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
    let recognised = rclone.recognises(&remote).expect("rclone's");
    rclone
        .disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(left);

    if let Some(package) = std::env::var_os("SKWIR_TEST_GVFS").map(PathBuf::from) {
        let private = PrivateGvfs::start(&package, &base);
        let gvfs = skwir_plugin_remote::gvfs::Gvfs {
            env: private.env.clone(),
            runtime: Some(private.runtime.clone()),
            ..Default::default()
        };
        let served_from = Address::parse(&format!(
            "dav://127.0.0.1:{}/remote.php/dav/files/me/",
            served.port
        ))
        .unwrap();
        let (connected, left) = opened(&gvfs, &served_from);
        let remote = skwir_core::fs::reach::of(&connected.point).unwrap();
        let recognised = gvfs.recognises(&remote).expect("GVfs's");
        gvfs.disconnect(&remote, &recognised, &CancelToken::new())
            .expect("disconnect");
        drop(left);
        drop(private);
    } else {
        eprintln!("GVfs skipped: SKWIR_TEST_GVFS names no GVfs package");
    }
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// KDE's network folders as Skwir writes them (`servers::add`, KDE's
/// chosen) read back through KIO's own `remote:/`, kio-extras' worker that
/// Dolphin's Network shows: each server listed, by the name given, leading
/// where its address does, its name and address holding what a desktop
/// entry must escape.
#[test]
fn kde_reads_the_network_folders_skwir_writes() {
    let Some(packages) = kio_packages() else {
        eprintln!("skipped: SKWIR_TEST_KIO names no KIO packages");
        return;
    };
    let Some(kioclient) = skwir_core::os_path::find_program("kioclient") else {
        eprintln!("skipped: kioclient (KDE's command-line tools) is not installed");
        return;
    };
    let base = fixture("kde-folders");
    let data = base.join("data");
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before the test starts an app, so no other thread reads the
    // environment meanwhile.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_DATA_HOME", &data);
    }
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins.add(
        Arc::new(RemotePlugin::with(Remotes::with(Vec::new()))),
        &app,
    );
    app.settings
        .set("remote.servers_kept_in", toml::Value::String("kde".into()));
    let servers = [
        (
            "It's shared",
            "dav://127.0.0.1:8080/sub folder",
            "webdav://127.0.0.1:8080/sub%20folder",
        ),
        (
            "Ünïcode files",
            "sftp://me@example.com:2222/srv/it's",
            "sftp://me@example.com:2222/srv/it's",
        ),
        (" Back\\slash", "smb://nas/photos", "smb://nas/photos"),
    ];
    for (name, address, _) in servers {
        let kept = skwir_plugin_remote::servers::add(&app, name, &Address::parse(address).unwrap())
            .unwrap();
        assert_eq!(kept, skwir_plugin_remote::servers::Kept::Kde, "{name}");
    }
    let private = PrivateKio::start(&packages, &base);
    let kio = |args: &[&str]| {
        let out = Command::new(&kioclient)
            .args(args)
            .envs(private.env.iter().map(|(k, v)| (k, v)))
            .env("XDG_DATA_HOME", &data)
            .output()
            .expect("kioclient runs");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // Listed by the name given (its entry's `Name`), whatever its file is
    // called; asked by it, where its address leads.
    let listed = kio(&["ls", "remote:/"]);
    let escaped = |name: &str| -> String {
        name.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    };
    let field = |stat: &str, key: &str| -> Option<String> {
        stat.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim_start().to_owned())
    };
    for (name, _, url) in servers {
        assert!(
            listed.lines().any(|l| l == name),
            "“{name}” listed: {listed}"
        );
        // kioclient's own command line refuses a URL whose path begins
        // with a space or holds a backslash ("Malformed URL"), which
        // Dolphin never types: those are listed alone.
        if name.starts_with(' ') || name.contains('\\') {
            continue;
        }
        let stat = kio(&["stat", &format!("remote:/{}", escaped(name))]);
        assert_eq!(field(&stat, "NAME").as_deref(), Some(name), "{stat}");
        assert_eq!(field(&stat, "TARGET_URL").as_deref(), Some(url), "{stat}");
    }
    let entries = std::fs::read_dir(data.join("remoteview")).unwrap().count();
    assert_eq!(entries, servers.len());
    drop(private);
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}
