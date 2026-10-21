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
use skwir_plugin_remote::connector::{Ask, Connector};
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
            .stderr(Stdio::null())
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
    if let Some(why) = Rclone.unavailable() {
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
    let remotes = Remotes::with(vec![Arc::new(Rclone)]);
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
    assert!(folder.ends_with("sub folder"), "{}", folder.display());
    let remote = skwir_core::fs::reach::of(&folder).expect("a remote's");
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
    assert_eq!(place.title, "127.0.0.1", "named by its server");
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

/// A mount point let go of when dropped, if the test left it mounted.
struct Unmounted(PathBuf);

impl Drop for Unmounted {
    fn drop(&mut self) {
        if skwir_core::fs::reach::of(&self.0).is_some_and(|r| r.point == self.0) {
            // Lazily: what the app still holds open there lets go after.
            let _ = Command::new("fusermount3").arg("-uz").arg(&self.0).status();
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
    if let Some(why) = Rclone.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("again");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("it's here.txt"), "here").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let remotes = Remotes::with(vec![Arc::new(Rclone)]);
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

/// Answers as the window would, and says what it was asked.
#[derive(Default)]
struct Answers {
    password: String,
    asked: Mutex<Vec<String>>,
}

impl Ask for Answers {
    fn secret(&self, title: &str, _: &str, _: &CancelToken) -> Option<String> {
        self.asked.lock().unwrap().push(format!("secret: {title}"));
        Some(self.password.clone())
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
    if let Some(why) = sshfs.unavailable().or_else(|| Rclone.unavailable()) {
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
        ..Answers::default()
    });
    let ask: Arc<dyn Ask> = answers.clone();
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", served.port)).unwrap();
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
            .any(|q| q.starts_with("secret:") && q.contains("password")),
        "its password was asked for: {asked:?}"
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
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// OpenSSH's own server, run as this user on a port of its own, with keys
/// and known hosts of the test's: rsync on its far end (`rclone serve
/// sftp` runs no command). Stopped when dropped.
struct OwnSshd {
    child: Child,
    port: u16,
    /// What ssh is given to reach it: the test's key and known hosts.
    options: Vec<String>,
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
        let child = Command::new(&sshd)
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
        Some(OwnSshd {
            child,
            port,
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
    if let Some(why) = Rclone.unavailable() {
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
        let _ = Command::new("fusermount3")
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
    if let Some(why) = Rclone.unavailable() {
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
        ..Answers::default()
    });
    let ask: Arc<dyn Ask> = answers.clone();
    let address = Address::parse(&format!("sftp://tester@127.0.0.1:{}/", served.port)).unwrap();
    // GVfs starts its bridge as its daemon starts: a moment after the bus.
    let connected = gvfs
        .connect(&address, &ask, &CancelToken::new())
        .expect("connect");
    let asked = answers.asked.lock().unwrap().clone();
    assert!(
        asked.iter().any(|q| q.starts_with("secret:")),
        "its password was asked for: {asked:?}"
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
    assert_eq!(recognised.label, "tester@127.0.0.1");

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
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}
