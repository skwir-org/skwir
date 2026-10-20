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
    if let Some(why) = Rclone.unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("sub folder")).unwrap();
    std::fs::write(served_root.join("hello.txt"), "hello").unwrap();
    let served = Served::start(&served_root);
    let address = Address::parse(&format!("dav://127.0.0.1:{}/sub folder", served.port)).unwrap();
    let ask: Arc<dyn Ask> = Arc::new(NoOne);
    let cancel = CancelToken::new();
    let connected = Rclone.connect(&address, &ask, &cancel).expect("connect");
    let point = connected.point.clone();
    let remote = skwir_core::fs::reach::of(&point).expect("a network mount");
    assert_eq!(remote.point, point);
    assert_eq!(remote.fstype, "fuse.rclone");
    assert!(!remote.by_kernel());
    let recognised = Rclone.recognises(&remote).expect("its own");
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

    Rclone
        .disconnect(&remote, &recognised, &cancel)
        .expect("disconnect");
    assert!(skwir_core::fs::reach::of(&point).is_none_or(|r| r.point != point));
    assert!(!point.exists(), "its mount point goes with it");
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}
