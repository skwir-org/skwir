//! A session bus of the test's own, and on it a stand-in of KDE's kmtpd
//! (ADR-097): `kiod6`'s name, the daemon, one device and one storage, its
//! files in memory, its copies ended by `copyFinished` as kmtpd's are; or
//! a `kiod6` whose kmtpd is not loaded (`Unloaded`), which says what was
//! addressed to it, as a call to it would load it.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, Read, Seek, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedFd, OwnedObjectPath};

pub const SERVICE: &str = "org.kde.kiod6";
pub const DAEMON: &str = "/modules/kmtpd";
pub const DEVICE: &str = "/modules/kmtpd/device0";
pub const STORAGE: &str = "/modules/kmtpd/device0/storage0";

/// A session bus of the test's own, stopped when dropped.
pub struct Bus {
    daemon: Child,
    pub address: String,
    _runtime: tempfile::TempDir,
}

impl Bus {
    /// `None` without `dbus-daemon`.
    pub fn start() -> Option<Bus> {
        // Short, as a real runtime folder is: a socket's path fits 108 bytes.
        let runtime = tempfile::Builder::new().prefix("k").tempdir().unwrap();
        let config = runtime.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                runtime.path().display()
            ),
        )
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut address = String::new();
        std::io::BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Some(Bus {
            daemon,
            address: address.trim().to_owned(),
            _runtime: runtime,
        })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// An entry of the stand-in's storage.
#[derive(Clone, Debug)]
pub struct Entry {
    pub folder: bool,
    pub data: Vec<u8>,
    pub id: u32,
}

/// The storage's entries by path (`/DCIM/a.jpg`).
#[derive(Default)]
pub struct Tree {
    pub entries: BTreeMap<String, Entry>,
    next: u32,
}

type Listed = (u32, u32, u32, String, u64, i64, String);

/// When every entry of the stand-in changed: a time a test can tell.
pub const MODIFIED: i64 = 1_700_000_000;

impl Tree {
    fn add(&mut self, path: &str, folder: bool, data: Vec<u8>) -> u32 {
        self.next += 1;
        self.entries.insert(
            path.to_owned(),
            Entry {
                folder,
                data,
                id: self.next,
            },
        );
        self.next
    }

    fn parent(path: &str) -> &str {
        match path.rfind('/') {
            Some(0) | None => "/",
            Some(at) => &path[..at],
        }
    }

    fn folder_exists(&self, path: &str) -> bool {
        path == "/" || self.entries.get(path).is_some_and(|e| e.folder)
    }

    fn listed(&self, path: &str) -> Option<Listed> {
        let entry = self.entries.get(path)?;
        let name = path.rsplit('/').next().unwrap_or_default().to_owned();
        Some((
            entry.id,
            0,
            1,
            name,
            entry.data.len() as u64,
            MODIFIED,
            if entry.folder {
                "inode/directory".to_owned()
            } else {
                "application/octet-stream".to_owned()
            },
        ))
    }

    fn children(&self, path: &str) -> Vec<Listed> {
        self.entries
            .keys()
            .filter(|p| Self::parent(p) == path && *p != path)
            .filter_map(|p| self.listed(p))
            .collect()
    }

    fn remove(&mut self, path: &str) {
        let inside = format!("{path}/");
        self.entries
            .retain(|p, _| p != path && !p.starts_with(&inside));
    }
}

struct Daemon(Arc<Mutex<Vec<OwnedObjectPath>>>);

#[zbus::interface(name = "org.kde.kmtp.Daemon")]
impl Daemon {
    #[zbus(name = "listDevices")]
    fn list_devices(&self) -> Vec<OwnedObjectPath> {
        self.0.lock().clone()
    }
}

struct Device {
    udi: String,
    name: String,
}

// zbus serves an interface's methods on its object: some answer alike.
#[allow(clippy::unused_self)]
#[zbus::interface(name = "org.kde.kmtp.Device")]
impl Device {
    #[zbus(name = "listStorages")]
    fn list_storages(&self) -> Vec<OwnedObjectPath> {
        vec![OwnedObjectPath::try_from(STORAGE).unwrap()]
    }

    #[zbus(property, name = "udi")]
    fn udi(&self) -> String {
        self.udi.clone()
    }

    #[zbus(property, name = "friendlyName")]
    fn friendly_name(&self) -> String {
        self.name.clone()
    }
}

struct Storage {
    tree: Arc<Mutex<Tree>>,
    bus: Arc<OnceLock<Connection>>,
}

impl Storage {
    /// A copy ended, said after its call answered, as kmtpd says it.
    fn finished(&self, result: i32) {
        let Some(bus) = self.bus.get().cloned() else {
            return;
        };
        std::thread::spawn(move || {
            let _ = bus.emit_signal(
                None::<&str>,
                STORAGE,
                "org.kde.kmtp.Storage",
                "copyFinished",
                &(result,),
            );
        });
    }
}

// zbus serves an interface's methods on its object: some answer alike.
#[allow(clippy::unused_self)]
#[zbus::interface(name = "org.kde.kmtp.Storage")]
impl Storage {
    #[zbus(property, name = "description")]
    fn description(&self) -> String {
        "Internal storage".to_owned()
    }

    #[zbus(property, name = "maxCapacity")]
    fn max_capacity(&self) -> u64 {
        64 << 30
    }

    #[zbus(property, name = "freeSpaceInBytes")]
    fn free_space_in_bytes(&self) -> u64 {
        16 << 30
    }

    #[zbus(name = "getFilesAndFolders", out_args("files", "exists"))]
    fn get_files_and_folders(&self, path: &str) -> (Vec<Listed>, i32) {
        let tree = self.tree.lock();
        if tree.folder_exists(path) {
            (tree.children(path), 0)
        } else if tree.entries.contains_key(path) {
            (Vec::new(), 2)
        } else {
            (Vec::new(), 1)
        }
    }

    #[zbus(name = "getFileMetadata")]
    fn get_file_metadata(&self, path: &str) -> Listed {
        self.tree
            .lock()
            .listed(path)
            .unwrap_or((0, 0, 0, String::new(), 0, 0, String::new()))
    }

    #[zbus(name = "getFileToFileDescriptor")]
    fn get_file_to_file_descriptor(&self, descriptor: OwnedFd, source_path: &str) -> i32 {
        let Some(data) = self
            .tree
            .lock()
            .entries
            .get(source_path)
            .filter(|e| !e.folder)
            .map(|e| e.data.clone())
        else {
            return 1;
        };
        let mut file = std::fs::File::from(std::os::fd::OwnedFd::from(descriptor));
        let written = file.write_all(&data).is_ok();
        self.finished(if written { 0 } else { -1 });
        0
    }

    #[zbus(name = "sendFileFromFileDescriptor")]
    fn send_file_from_file_descriptor(&self, descriptor: OwnedFd, destination_path: &str) -> i32 {
        let mut tree = self.tree.lock();
        if !tree.folder_exists(Tree::parent(destination_path)) {
            return 2;
        }
        // MTP replaces nothing (a phone would keep two of one name): what
        // is there must be removed first.
        if tree.entries.contains_key(destination_path) {
            return 3;
        }
        let mut file = std::fs::File::from(std::os::fd::OwnedFd::from(descriptor));
        let mut data = Vec::new();
        let _ = file.seek(std::io::SeekFrom::Start(0));
        if file.read_to_end(&mut data).is_err() {
            return 1;
        }
        tree.add(destination_path, false, data);
        drop(tree);
        self.finished(0);
        0
    }

    #[zbus(name = "setFileName")]
    fn set_file_name(&self, path: &str, new_name: &str) -> i32 {
        let mut tree = self.tree.lock();
        if !tree.entries.contains_key(path) {
            return 1;
        }
        let parent = Tree::parent(path);
        let renamed = if parent == "/" {
            format!("/{new_name}")
        } else {
            format!("{parent}/{new_name}")
        };
        let inside = format!("{path}/");
        let moved: Vec<(String, Entry)> = tree
            .entries
            .iter()
            .filter(|(p, _)| *p == path || p.starts_with(&inside))
            .map(|(p, e)| (format!("{renamed}{}", &p[path.len()..]), e.clone()))
            .collect();
        tree.remove(path);
        tree.entries.extend(moved);
        0
    }

    #[zbus(name = "createFolder")]
    fn create_folder(&self, path: &str) -> u32 {
        let mut tree = self.tree.lock();
        if tree.entries.contains_key(path) || !tree.folder_exists(Tree::parent(path)) {
            return 0;
        }
        tree.add(path, true, Vec::new())
    }

    #[zbus(name = "deleteObject")]
    fn delete_object(&self, path: &str) -> i32 {
        let mut tree = self.tree.lock();
        if !tree.entries.contains_key(path) {
            return 1;
        }
        tree.remove(path);
        0
    }
}

/// The stand-in on a bus: `kiod6`, kmtpd loaded, a phone.
pub struct Phone {
    pub tree: Arc<Mutex<Tree>>,
    bus: Connection,
    devices: Arc<Mutex<Vec<OwnedObjectPath>>>,
}

impl Phone {
    /// `kiod6` on the bus at `address`, kmtpd loaded, holding the phone
    /// `udi` named `name`, its storage holding `files` (a path ending in
    /// `/` a folder).
    pub fn serve(address: &str, udi: &str, name: &str, files: &[(&str, &[u8])]) -> Phone {
        let tree = Arc::new(Mutex::new(Tree::default()));
        {
            let mut tree = tree.lock();
            for (path, data) in files {
                match path.strip_suffix('/') {
                    Some(folder) => tree.add(folder, true, Vec::new()),
                    None => tree.add(path, false, data.to_vec()),
                };
            }
        }
        let devices = Arc::new(Mutex::new(vec![OwnedObjectPath::try_from(DEVICE).unwrap()]));
        let once = Arc::new(OnceLock::new());
        let bus = zbus::blocking::connection::Builder::address(address)
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(DAEMON, Daemon(Arc::clone(&devices)))
            .unwrap()
            .serve_at(
                DEVICE,
                Device {
                    udi: udi.to_owned(),
                    name: name.to_owned(),
                },
            )
            .unwrap()
            .serve_at(
                STORAGE,
                Storage {
                    tree: Arc::clone(&tree),
                    bus: Arc::clone(&once),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let _ = once.set(bus.clone());
        Phone { tree, bus, devices }
    }

    /// The phone unplugged: kmtpd lists it no more, and says so.
    pub fn unplug(&self) {
        self.devices.lock().clear();
        self.bus
            .emit_signal(
                None::<&str>,
                DAEMON,
                "org.kde.kmtp.Daemon",
                "devicesChanged",
                &(),
            )
            .unwrap();
    }

    /// What the storage holds at `path`: a file's bytes.
    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.tree
            .lock()
            .entries
            .get(path)
            .filter(|e| !e.folder)
            .map(|e| e.data.clone())
    }

    /// Whether the storage holds a folder at `path`.
    pub fn folder(&self, path: &str) -> bool {
        self.tree.lock().folder_exists(path)
    }
}

/// A `kiod6` on a bus whose kmtpd is not loaded: it answers what `/modules`
/// holds (nothing), and says which calls were addressed to kmtpd, each of
/// which a real `kiod6` would have loaded it for.
pub struct Unloaded {
    _bus: Connection,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Unloaded {
    pub fn serve(address: &str) -> Unloaded {
        let bus = zbus::blocking::connection::Builder::address(address)
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .build()
            .unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let calls = zbus::blocking::MessageIterator::from(&bus);
        let (answering, noted) = (bus.clone(), Arc::clone(&asked));
        std::thread::spawn(move || {
            for message in calls {
                let Ok(message) = message else { return };
                let header = message.header();
                if header.message_type() != zbus::message::Type::MethodCall {
                    continue;
                }
                let path = header
                    .path()
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_default();
                if path.starts_with(DAEMON) {
                    noted.lock().push(path);
                    let _ = answering.reply_error(
                        &header,
                        "org.freedesktop.DBus.Error.UnknownObject",
                        &("not loaded",),
                    );
                } else if path == "/modules" && header.member().is_some_and(|m| m == "Introspect") {
                    let _ = answering.reply(
                        &header,
                        &("<node><interface name=\"org.freedesktop.DBus.Introspectable\"/></node>",),
                    );
                }
            }
        });
        Unloaded { _bus: bus, asked }
    }

    /// The paths of kmtpd's addressed so far.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().clone()
    }
}

/// A folder of the test's on the home's file system, never `/tmp`.
pub fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests/kmtp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// Whether FUSE can be mounted here by a user.
pub fn fuse() -> bool {
    std::path::Path::new("/dev/fuse").exists()
        && std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|d| d.join("fusermount3").is_file())
        })
}

/// Whether a FUSE mount of type `fuse.skwir-kmtp` is at `point`.
pub fn mounted(point: &std::path::Path) -> bool {
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap_or_default()
        .lines()
        .any(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let at = fields.get(4).copied().unwrap_or_default();
            let kind = line.split(" - ").nth(1).unwrap_or_default();
            std::path::Path::new(at) == point && kind.starts_with("fuse.skwir-kmtp ")
        })
}

/// Wait at most ten seconds until `done`.
pub fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Let go of `point` if still mounted: a failed test leaves no mount.
pub struct Unmounted(pub PathBuf);

impl Drop for Unmounted {
    fn drop(&mut self) {
        if mounted(&self.0) {
            let _ = Command::new("fusermount3").arg("-uz").arg(&self.0).status();
        }
    }
}
