//! The helper against a stand-in kmtpd on a bus of the test's own, mounted
//! for real through FUSE (ADR-097): the phone's storage a folder, read,
//! written, made, renamed and removed through it as the stand-in sees,
//! what MTP cannot do refused; let go of when the phone leaves kmtpd.
//! Skipped, saying why, without FUSE or `dbus-daemon`.
#![cfg(target_os = "linux")]

mod common;

use std::path::Path;
use std::process::Command;

use common::{Bus, Phone, Unmounted, fixture, fuse, mounted, until};

const WORKER: &str = env!("CARGO_BIN_EXE_skwir-kmtp-worker");
const UDI: &str = "/org/kde/solid/udev/sys/devices/pci0000:00/usb9/9-1";

fn mount(bus: &Bus, point: &Path, cache: &Path, udi: &str) -> std::process::Output {
    Command::new(WORKER)
        .arg("mount")
        .arg(udi)
        .arg(point)
        .arg(cache)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .output()
        .unwrap()
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_phone_kmtpd_holds_is_a_folder_until_unplugged() {
    if !fuse() {
        eprintln!("skipped: no FUSE here");
        return;
    }
    let Some(bus) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let phone = Phone::serve(
        &bus.address,
        UDI,
        "Example Phone",
        &[
            ("/DCIM/", b""),
            ("/DCIM/It's 1.jpg", b"a picture"),
            ("/Music/", b""),
            ("/Music/Old/", b""),
            ("/notes.txt", b"old notes"),
        ],
    );
    let base = fixture("folder");
    let (point, cache) = (base.join("point"), base.join("cache"));
    std::fs::create_dir_all(&point).unwrap();
    let mounting = mount(&bus, &point, &cache, UDI);
    assert!(
        mounting.status.success(),
        "{}",
        String::from_utf8_lossy(&mounting.stderr)
    );
    let _unmounted = Unmounted(point.clone());
    assert!(mounted(&point), "its type is the device's");

    // Each storage a folder, by its description.
    assert_eq!(names(&point), ["Internal storage"]);
    let storage = point.join("Internal storage");
    assert_eq!(names(&storage), ["DCIM", "Music", "notes.txt"]);
    let picture = storage.join("DCIM/It's 1.jpg");
    assert_eq!(std::fs::read(&picture).unwrap(), b"a picture");
    let meta = std::fs::metadata(&picture).unwrap();
    assert_eq!(meta.len(), 9);
    assert_eq!(
        meta.modified().unwrap(),
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(common::MODIFIED as u64),
        "the time the phone says"
    );

    // Written: sent whole once closed.
    std::fs::write(storage.join("DCIM/new é.txt"), b"made here").unwrap();
    assert_eq!(
        phone.file("/DCIM/new é.txt").as_deref(),
        Some(&b"made here"[..])
    );
    // Replaced: what was there removed first, as MTP replaces nothing.
    std::fs::write(storage.join("notes.txt"), b"new notes, longer").unwrap();
    assert_eq!(
        phone.file("/notes.txt").as_deref(),
        Some(&b"new notes, longer"[..])
    );
    assert_eq!(
        std::fs::read(storage.join("notes.txt")).unwrap(),
        b"new notes, longer"
    );
    // Appended to: fetched, grown, sent.
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(storage.join("notes.txt"))
            .unwrap();
        file.write_all(b"!").unwrap();
    }
    assert_eq!(
        phone.file("/notes.txt").as_deref(),
        Some(&b"new notes, longer!"[..])
    );

    // Folders made, renamed in place, removed when empty.
    std::fs::create_dir(storage.join("Made")).unwrap();
    assert!(phone.folder("/Made"));
    std::fs::rename(storage.join("Made"), storage.join("Made again")).unwrap();
    assert!(phone.folder("/Made again") && !phone.folder("/Made"));
    std::fs::rename(storage.join("Music"), storage.join("Songs")).unwrap();
    assert!(phone.folder("/Songs/Old"), "what it holds goes with it");
    assert_eq!(names(&storage.join("Songs")), ["Old"]);
    let refused = std::fs::remove_dir(storage.join("Songs")).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::DirectoryNotEmpty);
    std::fs::remove_dir(storage.join("Songs/Old")).unwrap();
    std::fs::remove_dir(storage.join("Songs")).unwrap();
    assert!(!phone.folder("/Songs"));
    std::fs::remove_file(storage.join("DCIM/new é.txt")).unwrap();
    assert!(phone.file("/DCIM/new é.txt").is_none());

    // What MTP cannot do is refused: a move between folders (a copy is
    // written where it goes), times and modes (the phone stamps).
    let moved = std::fs::rename(picture.clone(), storage.join("It's 1.jpg")).unwrap_err();
    assert_eq!(moved.kind(), std::io::ErrorKind::CrossesDevices);
    let file = std::fs::File::options().write(true).open(&picture).unwrap();
    let stamped = file.set_modified(std::time::SystemTime::now()).unwrap_err();
    assert_eq!(stamped.kind(), std::io::ErrorKind::Unsupported);
    drop(file);
    assert_eq!(
        phone.file("/DCIM/It's 1.jpg").as_deref(),
        Some(&b"a picture"[..])
    );

    // How full it is, as the phone says.
    let space = rustix::fs::statvfs(&storage).unwrap();
    assert_eq!(space.f_blocks * space.f_frsize, 64 << 30);
    assert_eq!(space.f_bavail * space.f_frsize, 16 << 30);

    // Nothing kept in the cache once closed.
    assert!(std::fs::read_dir(&cache).unwrap().next().is_none());

    // Unplugged: kmtpd lists it no more, and the mount goes.
    phone.unplug();
    until("the mount let go of", || !mounted(&point));
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_device_kmtpd_does_not_hold_is_not_mounted() {
    if !fuse() {
        eprintln!("skipped: no FUSE here");
        return;
    }
    let Some(bus) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let _phone = Phone::serve(&bus.address, UDI, "Example Phone", &[]);
    let base = fixture("not-held");
    let point = base.join("point");
    std::fs::create_dir_all(&point).unwrap();
    let mounting = mount(&bus, &point, &base.join("cache"), "/another/udi");
    let _unmounted = Unmounted(point.clone());
    assert!(!mounting.status.success());
    assert_eq!(
        String::from_utf8_lossy(&mounting.stderr).trim(),
        "KDE's kmtpd does not hold this device"
    );
    assert!(!mounted(&point));
    std::fs::remove_dir_all(&base).unwrap();
}

/// Given up on before it mounted (kmtpd not answering: here its bus
/// stopped), the helper's process group is stopped whole, as
/// `process::run_leaving` stops it: what it started to serve the mount
/// ends with it, mounting nothing later.
#[test]
fn given_up_on_before_it_mounted_it_leaves_nothing_running() {
    use std::os::unix::process::CommandExt;
    if !fuse() {
        eprintln!("skipped: no FUSE here");
        return;
    }
    let Some(bus) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let _phone = Phone::serve(&bus.address, UDI, "Example Phone", &[]);
    let base = fixture("given-up");
    let point = base.join("point");
    std::fs::create_dir_all(&point).unwrap();
    let _unmounted = Unmounted(point.clone());
    let signal = |args: &[&str]| {
        let sent = Command::new("kill").args(args).status().unwrap();
        assert!(sent.success(), "kill {args:?}");
    };
    let daemon = bus.daemon_pid().to_string();
    signal(&["-STOP", &daemon]);
    let mut helper = Command::new(WORKER)
        .arg("mount")
        .arg(UDI)
        .arg(&point)
        .arg(base.join("cache"))
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    // Its server started, waiting on the bus.
    let children = format!("/proc/{0}/task/{0}/children", helper.id());
    let mut serving = String::new();
    until("it started its server", || {
        serving = std::fs::read_to_string(&children)
            .unwrap_or_default()
            .trim()
            .to_owned();
        !serving.is_empty()
    });
    signal(&["-KILL", "--", &format!("-{}", helper.id())]);
    let _ = helper.wait();
    // Gone, or ended and not yet collected by whoever took it in.
    let ended = || {
        std::fs::read_to_string(format!("/proc/{serving}/stat")).map_or(true, |stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, rest)| rest.starts_with('Z'))
        })
    };
    until("its server ended with it", ended);
    signal(&["-CONT", &daemon]);
    assert!(!mounted(&point));
    std::fs::remove_dir_all(&base).unwrap();
}

/// Run as root in a user namespace (as `just mtp-check` runs it), the
/// helper mounts with the type Skwir knows it by too (`fuse.skwir-kmtp`):
/// fuser's own mount as root names it `fuse` alone. The test above, run
/// again there. Skipped where user namespaces are not allowed.
#[test]
fn as_root_in_a_user_namespace_too() {
    if std::env::var_os("SKWIR_TEST_IN_NAMESPACE").is_some() || !fuse() {
        return;
    }
    let Ok(test) = std::env::current_exe() else {
        return;
    };
    let ran = Command::new("unshare")
        .args(["-r", "-m", "--propagation", "private"])
        .arg(&test)
        .args(["--exact", "a_phone_kmtpd_holds_is_a_folder_until_unplugged"])
        .env("SKWIR_TEST_IN_NAMESPACE", "1")
        .output();
    let Ok(ran) = ran else {
        eprintln!("skipped: unshare is not installed");
        return;
    };
    let said = String::from_utf8_lossy(&ran.stderr);
    if !ran.status.success() && said.contains("unshare failed") {
        eprintln!("skipped: user namespaces are not allowed here: {said}");
        return;
    }
    assert!(
        ran.status.success(),
        "{}\n{said}",
        String::from_utf8_lossy(&ran.stdout)
    );
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("1 passed"),
        "it ran: {}",
        String::from_utf8_lossy(&ran.stdout)
    );
}
