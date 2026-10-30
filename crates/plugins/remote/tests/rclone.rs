//! rclone's connector against a server on this machine (`rclone serve
//! webdav`, a folder of the test's), mounted for real through FUSE: an
//! address connects, the folder lists, a copy is on the server once done,
//! and it is let go of. Skipped, saying why, where rclone or FUSE is
//! missing.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::cancel::CancelToken;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::{Ask, Connector};
use skwir_plugin_remote::rclone::Rclone;

/// Answers nothing: what is tested asks nothing.
struct NoOne;

impl Ask for NoOne {
    fn secret(&self, _: &str, _: &str, _: &CancelToken) -> Option<String> {
        None
    }
    fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
        false
    }
}

/// A WebDAV server serving `root` on a port of its own, stopped when
/// dropped.
struct Served {
    child: Child,
    port: u16,
}

impl Served {
    fn start(root: &Path) -> Served {
        let port = free_port();
        let child = Command::new("rclone")
            .args(["serve", "webdav"])
            .arg(root)
            .args(["--addr", &format!("127.0.0.1:{port}")])
            .arg("--config")
            .arg(root.with_extension("conf"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start rclone serve");
        let deadline = Instant::now() + Duration::from_secs(10);
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

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// What lets go of a FUSE mount here: the system's setuid wrapper where
/// it has one (NixOS: the store's `fusermount3` may not unmount), else
/// the one on the `PATH`.
fn fusermount() -> PathBuf {
    let wrapper = Path::new("/run/wrappers/bin/fusermount3");
    if wrapper.exists() {
        wrapper.to_path_buf()
    } else {
        PathBuf::from("fusermount3")
    }
}

/// Whether the mount table names a mount at `point`, its spaces as the
/// table writes them.
fn mounted_at(point: &Path) -> bool {
    let written = point.to_string_lossy().replace(' ', "\\040");
    std::fs::read_to_string("/proc/self/mountinfo").is_ok_and(|table| {
        table
            .lines()
            .any(|line| line.split(' ').nth(4) == Some(written.as_str()))
    })
}

/// A mount the test made at `point`: let go of when dropped, whether the
/// test ended or failed midway (a mount whose program is gone holds
/// whatever looks into it, the next tests included), the `rclone mount`
/// the test started there (`child`) stopped after.
struct Mounted {
    child: Option<Child>,
    point: PathBuf,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        if mounted_at(&self.point) {
            let _ = Command::new(fusermount())
                .arg("-uz")
                .arg(&self.point)
                .status();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

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

#[test]
fn an_address_rclone_opens_is_a_folder_and_a_copy_is_on_the_server_once_done() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("sub folder")).unwrap();
    std::fs::write(served_root.join("hello.txt"), "hello").unwrap();
    let served = Served::start(&served_root);
    // The server's root: a WebDAV share is mounted where its address says.
    let address = Address::parse(&format!("dav://127.0.0.1:{}/", served.port)).unwrap();
    let ask: Arc<dyn Ask> = Arc::new(NoOne);
    let cancel = CancelToken::new();
    let connected = Rclone::default()
        .connect(&address, &ask, &cancel)
        .expect("connect");
    let point = connected.point.clone();
    let left = Mounted {
        child: None,
        point: point.clone(),
    };
    let remote = skwir_core::fs::reach::of(&point).expect("a network mount");
    assert_eq!(remote.point, point);
    assert_eq!(remote.fstype, "fuse.rclone");
    assert!(!remote.by_kernel());
    let recognised = Rclone::default().recognises(&remote).expect("its own");
    assert!(recognised.ours);
    assert_eq!(recognised.address.as_ref(), Some(&address.top()));

    // The folder lists what the server holds.
    let mut names: Vec<String> = std::fs::read_dir(&point)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["hello.txt", "sub folder"]);

    // A file written alone is on the server once closed.
    let written = point.join("sub folder").join("written.bin");
    std::fs::write(&written, vec![7u8; 1 << 20]).unwrap();
    let on_server = served_root.join("sub folder").join("written.bin");
    assert_eq!(
        std::fs::metadata(&on_server).map(|m| m.len()).ok(),
        Some(1 << 20),
        "on the server as soon as written"
    );

    Rclone::default()
        .disconnect(&remote, &recognised, &cancel)
        .expect("disconnect");
    assert!(skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point));
    assert!(!point.exists(), "its mount point goes with it");
    drop(left);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// `served`'s folder mounted at `base/mounted` with rclone's `full` cache,
/// its remote control on the loopback behind a login of its own, as
/// Windows' mounts serve theirs: what is written kept until sent now, never
/// sent by itself while the test looks (`--vfs-write-back`).
#[cfg(target_os = "linux")]
fn mounted_full(base: &Path, served: &Served) -> (skwir_plugin_remote::rc::At, Mounted) {
    use skwir_plugin_remote::rc::At;
    let point = base.join("mounted");
    std::fs::create_dir_all(&point).unwrap();
    let at = At::loopback().unwrap();
    let (serving, login) = at.serving();
    let child = Command::new("rclone")
        .arg("mount")
        .arg(format!(":webdav,url='http://127.0.0.1:{}/':", served.port))
        .arg(&point)
        .args(["--vfs-cache-mode", "full", "--cache-dir"])
        .arg(base.join("cache"))
        .args(["--vfs-write-back", "10m"])
        .arg("--config")
        .arg(base.join("rclone.conf"))
        .arg("--rc")
        .args(serving)
        .envs(login)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start rclone mount");
    let mount = Mounted {
        child: Some(child),
        point: point.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while !(at.answers() && mounted_at(&point)) {
        assert!(Instant::now() < deadline, "never mounted");
        std::thread::sleep(Duration::from_millis(50));
    }
    (at, mount)
}

/// What a mount keeps and sends after a file is closed (`full`) is sent now
/// and waited for through its remote control on the loopback, behind a
/// login of its own, as Windows' mounts serve theirs (ADR-102): the calls
/// a socket takes on Unix, checked here on a mount of this machine.
#[cfg(target_os = "linux")]
#[test]
fn what_a_mount_sends_later_is_sent_through_a_control_on_the_loopback() {
    use skwir_plugin_remote::uploads;
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("sends-loopback");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start(&served_root);
    let (at, mount) = mounted_full(&base, &served);
    let point = mount.point.clone();
    let cancel = CancelToken::new();
    assert_eq!(uploads::cache_mode(&at, &cancel).unwrap(), "full");

    // Kept on this disk, sent after its close: waiting, until sent now.
    // Queued once rclone has the file's release, which the kernel sends
    // after `close` returns, on its own time.
    std::fs::write(point.join("l'été — a note.txt"), "sent").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while uploads::waiting(&at, &cancel).unwrap() == 0 {
        assert!(Instant::now() < deadline, "never waiting to be sent");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(uploads::waiting(&at, &cancel).unwrap(), 1);
    assert!(
        !served_root.join("l'été — a note.txt").exists(),
        "not sent by itself"
    );
    let mut told = Vec::new();
    uploads::send_now(&at, &cancel, &|| {}, &mut |left| told.push(left)).unwrap();
    assert_eq!(
        std::fs::read_to_string(served_root.join("l'été — a note.txt")).unwrap(),
        "sent",
        "on the server once sent"
    );
    assert_eq!(told.last(), Some(&0), "said as it went: {told:?}");
    assert_eq!(uploads::waiting(&at, &cancel).unwrap(), 0);

    drop(mount);
    assert!(!mounted_at(&point), "let go of");
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// What a plan wrote is waited for, not what waits when asked (ADR-102): a
/// file whose release has not reached rclone is in no queue yet (held open
/// here, as the kernel holds a release it has not sent), and the wait ends
/// only once the server has it too, with the others: a file, a folder's.
#[cfg(target_os = "linux")]
#[test]
fn what_was_written_is_sent_though_rclone_had_not_queued_it() {
    use skwir_plugin_remote::uploads;
    use std::io::Write;
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("sends-written");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start(&served_root);
    let (at, mount) = mounted_full(&base, &served);
    let point = mount.point.clone();
    std::fs::write(point.join("l'été — one.txt"), "one").unwrap();
    std::fs::create_dir_all(point.join("a \"folder\"/inner")).unwrap();
    std::fs::write(point.join("a \"folder\"/inner/two.txt"), "two").unwrap();
    let mut held = std::fs::File::create(point.join("held open.txt")).unwrap();
    held.write_all(b"held").unwrap();
    held.flush().unwrap();
    let written = vec![
        point.join("l'été — one.txt"),
        point.join("a \"folder\""),
        point.join("held open.txt"),
    ];
    let (told, heard) = std::sync::mpsc::channel();
    let sending = {
        let (at, point) = (at.clone(), point.clone());
        std::thread::spawn(move || {
            uploads::send_written(
                &at,
                &point,
                &written,
                &CancelToken::new(),
                &|| {},
                &mut |left| {
                    let _ = told.send(left);
                },
            )
        })
    };
    // The others sent, the one held open waits, unsent, not queued.
    loop {
        let left = heard
            .recv_timeout(Duration::from_secs(60))
            .expect("the wait ended with a file written still unsent");
        if left == 1 {
            break;
        }
    }
    assert!(!sending.is_finished(), "waiting for the file held open");
    assert!(!served_root.join("held open.txt").exists());
    assert_eq!(
        std::fs::read_to_string(served_root.join("a \"folder\"/inner/two.txt")).unwrap(),
        "two"
    );
    // Closed: its release reaches rclone, which queues it, and it is sent.
    drop(held);
    sending.join().unwrap().unwrap();
    assert_eq!(heard.try_iter().last(), Some(0), "said as it went");
    for (name, text) in [
        ("l'été — one.txt", "one"),
        ("a \"folder\"/inner/two.txt", "two"),
        ("held open.txt", "held"),
    ] {
        assert_eq!(
            std::fs::read_to_string(served_root.join(name))
                .ok()
                .as_deref(),
            Some(text),
            "{name} on the server once the wait ended"
        );
    }
    assert_eq!(uploads::waiting(&at, &CancelToken::new()).unwrap(), 0);
    drop(mount);
    assert!(!mounted_at(&point), "let go of");
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}
