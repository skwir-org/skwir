//! The devices GVfs offers (ADR-096), against a stand-in of its MTP volume
//! monitor on a bus of the test's own: a phone plugged in is listed among
//! the sidebar's devices by the name its volume has, at its address, gone
//! once unplugged, followed again once the plugin is enabled again; one
//! another program holds is said to be held, and by whom. Linux only, as
//! GVfs is. Skipped, saying why, without `dbus-daemon` or `gio`.
#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::app::{App, AppConfig};
use skwir_core::cancel::CancelToken;
use skwir_core::registry::Place;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::{Ask, Connector};
use skwir_plugin_remote::gvfs::Gvfs;
use skwir_plugin_remote::{RemotePlugin, Remotes};
use zbus::zvariant::OwnedValue;

const MONITOR: &str = "org.gtk.vfs.MTPVolumeMonitor";
const PATH: &str = "/org/gtk/Private/RemoteVolumeMonitor";
const INTERFACE: &str = "org.gtk.Private.RemoteVolumeMonitor";

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

/// A session bus of the test's own, stopped when dropped.
struct Bus {
    daemon: Child,
    address: String,
    _runtime: tempfile::TempDir,
}

impl Bus {
    fn start() -> Option<Bus> {
        // Short, as a real runtime folder is: a socket's path fits 108 bytes.
        let runtime = tempfile::Builder::new().prefix("d").tempdir().unwrap();
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

/// A volume the stand-in lists: its name, its address, its node.
#[derive(Clone)]
struct Volume {
    name: &'static str,
    uri: &'static str,
    node: PathBuf,
    mounts: bool,
}

/// A drive as GVfs's monitors send one (seventeen fields, more than a
/// tuple holds): none is listed, but its type is the answer's.
// Its fields are GVfs's, eight of them flags.
#[allow(clippy::struct_excessive_bools)]
#[derive(serde::Serialize, zbus::zvariant::Type)]
struct Drive {
    id: String,
    name: String,
    icon: String,
    symbolic_icon: String,
    removable: bool,
    media_removable: bool,
    has_media: bool,
    media_check_automatic: bool,
    can_poll_for_media: bool,
    can_eject: bool,
    can_start: bool,
    can_stop: bool,
    start_stop_type: u32,
    volumes: Vec<String>,
    identifiers: HashMap<String, String>,
    sort_key: String,
    expansion: HashMap<String, OwnedValue>,
}

/// A volume as GVfs's monitors send one.
type Sent = (
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    bool,
    String,
    String,
    HashMap<String, String>,
    String,
    HashMap<String, OwnedValue>,
);

/// A mount as GVfs's monitors send one.
type Mount = (
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    String,
    Vec<String>,
    String,
    HashMap<String, OwnedValue>,
);

fn sent(volume: &Volume) -> Sent {
    (
        format!("0x{:x}", volume.name.len()),
        volume.name.to_owned(),
        "phone".to_owned(),
        ". GThemedIcon phone-symbolic phone".to_owned(),
        String::new(),
        volume.uri.to_owned(),
        volume.mounts,
        true,
        String::new(),
        String::new(),
        HashMap::from([("unix-device".to_owned(), volume.node.display().to_string())]),
        String::new(),
        HashMap::new(),
    )
}

/// GVfs's MTP volume monitor, as the test says what is plugged in.
struct StandIn(Arc<Mutex<Vec<Volume>>>);

#[zbus::interface(name = "org.gtk.Private.RemoteVolumeMonitor")]
impl StandIn {
    #[zbus(out_args("drives", "volumes", "mounts"))]
    fn list(&self) -> (Vec<Drive>, Vec<Sent>, Vec<Mount>) {
        (
            Vec::new(),
            self.0.lock().iter().map(sent).collect(),
            Vec::new(),
        )
    }
}

/// Plug `volume` in (`VolumeAdded`) or unplug it (`VolumeRemoved`), as the
/// monitor tells its listeners.
fn plug(bus: &zbus::blocking::Connection, volumes: &Mutex<Vec<Volume>>, volume: &Volume, on: bool) {
    let mut listed = volumes.lock();
    listed.retain(|v| v.uri != volume.uri);
    if on {
        listed.push(volume.clone());
    }
    drop(listed);
    let signal = if on { "VolumeAdded" } else { "VolumeRemoved" };
    bus.emit_signal(
        None::<&str>,
        PATH,
        INTERFACE,
        signal,
        &(MONITOR, "stand-in", sent(volume)),
    )
    .unwrap();
}

fn devices(app: &App) -> Vec<Place> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|p| p.group == "devices")
        .collect()
}

/// Wait, at most ten seconds, until `done` says so of the devices listed.
fn until(app: &App, what: &str, done: impl Fn(&[Place]) -> bool) -> Vec<Place> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = devices(app);
        if done(&listed) {
            return listed;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: {:?}",
            listed.iter().map(|p| &p.title).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn found(listed: &[Place], name: &str) -> bool {
    listed.iter().any(|p| p.title == name)
}

#[test]
fn a_device_gvfs_offers_is_listed_until_unplugged_and_said_held() {
    if skwir_core::os_path::find_program("gio").is_none() {
        eprintln!("skipped: gio (GLib) is not installed");
        return;
    }
    let Some(private) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let base = tempfile::tempdir().unwrap();
    // The node a program holding the phone keeps open: a file of the test's.
    let node = base.path().join("usb-009-012");
    std::fs::write(&node, b"").unwrap();
    let phone = Volume {
        name: "Example Phone",
        uri: "mtp://Example_Phone_0123/",
        node: node.clone(),
        mounts: true,
    };
    let camera = Volume {
        name: "Example Camera",
        uri: "gphoto2://%5Busb%3A001%2C004%5D/",
        node: base.path().join("usb-001-004"),
        mounts: true,
    };
    let volumes = Arc::new(Mutex::new(vec![phone.clone()]));
    let monitor = zbus::blocking::connection::Builder::address(private.address.as_str())
        .unwrap()
        .name(MONITOR)
        .unwrap()
        .serve_at(PATH, StandIn(Arc::clone(&volumes)))
        .unwrap()
        .build()
        .unwrap();

    let runtime = tempfile::Builder::new().prefix("g").tempdir().unwrap();
    let gvfs: Arc<dyn Connector> = Arc::new(Gvfs {
        env: vec![(
            "DBUS_SESSION_BUS_ADDRESS".into(),
            private.address.clone().into(),
        )],
        runtime: Some(runtime.path().to_path_buf()),
        ..Gvfs::default()
    });
    let remotes = Remotes::with(vec![gvfs]);
    let app = App::new(AppConfig::ephemeral(base.path().join("app"))).unwrap();
    app.plugins
        .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);

    // Plugged in before: listed by its volume's name, at its address, not
    // mounted until opened.
    let listed = until(&app, "the phone plugged in", |d| found(d, "Example Phone"));
    let place = listed.iter().find(|p| p.title == "Example Phone").unwrap();
    assert_eq!(
        place.location,
        Address::parse(phone.uri).unwrap().location()
    );
    assert_eq!(place.icon.as_deref(), Some("phone"));
    assert!(place.unavailable.is_some(), "drawn as not mounted");
    assert!(place.eject.is_none(), "nothing to let go of");
    assert_eq!(
        place.key.as_deref(),
        Some("remote:mtp://Example_Phone_0123/")
    );
    assert_eq!(
        remotes.label_of(&Address::parse("mtp://Example_Phone_0123/DCIM").unwrap()),
        "Example Phone",
        "named so below its top too, never by its serial"
    );

    // Another plugged in, told: listed too, as a camera.
    plug(&monitor, &volumes, &camera, true);
    let listed = until(&app, "the camera plugged in", |d| {
        found(d, "Example Camera")
    });
    let place = listed.iter().find(|p| p.title == "Example Camera").unwrap();
    assert_eq!(place.icon.as_deref(), Some("camera-photo"));
    assert_eq!(
        place.location.to_string(),
        "gphoto2://[usb:001,004]/",
        "its address as Skwir writes it"
    );

    // A phone another program holds: opening it says which.
    let mut holder = Command::new("sleep")
        .arg("60")
        .stdin(std::fs::File::open(&node).unwrap())
        .spawn()
        .unwrap();
    let error = remotes
        .reach(
            &app,
            &Address::parse(phone.uri).unwrap(),
            &(Arc::new(NoOne) as Arc<dyn Ask>),
            &CancelToken::new(),
        )
        .unwrap_err()
        .to_string();
    let _ = holder.kill();
    let _ = holder.wait();
    assert_eq!(
        error,
        "sleep has it open, and only one program at a time can use a phone. Quit sleep, then try again."
    );

    // Unplugged, told: gone.
    plug(&monitor, &volumes, &phone, false);
    until(&app, "the phone unplugged", |d| !found(d, "Example Phone"));

    // Disabled, nothing listed; enabled again, what is plugged in is
    // followed again.
    assert!(app.plugins.set_enabled("remote", false, &app));
    until(&app, "the plugin disabled", |d| !found(d, "Example Camera"));
    plug(&monitor, &volumes, &phone, true);
    assert!(app.plugins.set_enabled("remote", true, &app));
    until(&app, "the plugin enabled again", |d| {
        found(d, "Example Phone") && found(d, "Example Camera")
    });

    // One whose volume does not mount, or names no device, is no device:
    // both listed by the monitor before it tells of a change.
    {
        let mut listed = volumes.lock();
        listed.retain(|v| v.uri != phone.uri);
        listed.push(Volume {
            mounts: false,
            ..phone.clone()
        });
        listed.push(Volume {
            name: "Not a device",
            uri: "sftp://example.com/",
            node: PathBuf::from("/nonexistent"),
            mounts: true,
        });
    }
    monitor
        .emit_signal(
            None::<&str>,
            PATH,
            INTERFACE,
            "VolumeChanged",
            &(MONITOR, "stand-in", sent(&phone)),
        )
        .unwrap();
    let listed = until(&app, "what is no device left out", |d| {
        !found(d, "Example Phone")
    });
    assert!(!found(&listed, "Not a device"));
    assert!(found(&listed, "Example Camera"));
}
