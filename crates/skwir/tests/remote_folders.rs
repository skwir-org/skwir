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
            let _ = Command::new("fusermount3").arg("-u").arg(&self.0).status();
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
    gvfs.disconnect(&remote, &recognised, &CancelToken::new())
        .expect("disconnect");
    drop(private);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}
