//! The Linux backend against stand-in servers (a notification server,
//! logind, the portal, a tray's watcher, a session manager) on a bus
//! daemon of the tests' own: never the session's or the system's, its
//! tray or its inhibitors.

use super::*;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::Mutex;
use skwir_core::CancelToken;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const ICON: &[u8] = include_bytes!("../../../ui/assets/org.skwir.skwir.svg");
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
/// Where the stand-in portal's requests are served.
const REQUESTS: &str = "/org/freedesktop/portal/desktop/request/test";

/// A bus daemon of the test's own, in a folder of its own under the
/// tests' cache: never the session's or the system's. Stopped when
/// dropped.
struct Daemon {
    child: std::process::Child,
    address: String,
    _folder: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `None` without `dbus-daemon`.
fn daemon() -> Option<Daemon> {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    let folder = tempfile::Builder::new()
        .prefix("desktop-")
        .tempdir_in(base)
        .unwrap();
    let config = folder.path().join("bus.conf");
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
            folder.path().display()
        ),
    )
    .unwrap();
    let mut child = std::process::Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut address = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    Some(Daemon {
        child,
        address: address.trim().to_owned(),
        _folder: folder,
    })
}

fn connect(address: &str) -> zbus::Result<Connection> {
    zbus::blocking::connection::Builder::address(address)?.build()
}

fn until(what: &str, done: &dyn Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Debug)]
struct Notified {
    app_name: String,
    replaces: u32,
    icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: HashMap<String, OwnedValue>,
    timeout: i32,
    id: u32,
}

impl Notified {
    fn hint(&self, key: &str) -> Option<&OwnedValue> {
        self.hints.get(key)
    }
}

/// What the stand-in servers were asked, and the ends they kept of
/// the descriptors they handed out.
#[derive(Default)]
struct Seen {
    notified: Vec<Notified>,
    closed: Vec<u32>,
    inhibited: Vec<[String; 4]>,
    held: Vec<UnixStream>,
    /// The portal's inhibitions: the window, the flags, the reason and
    /// the token asked.
    portal: Vec<(String, u32, String, String)>,
    /// The portal's requests closed, by path.
    requests_closed: Vec<String>,
}

/// A notification server, as the specification has it.
struct Notifications {
    seen: Arc<Mutex<Seen>>,
    next: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
#[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
impl Notifications {
    #[allow(clippy::too_many_arguments)] // The specification's eight.
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 {
            self.next.fetch_add(1, Ordering::SeqCst)
        } else {
            replaces_id
        };
        self.seen.lock().notified.push(Notified {
            app_name,
            replaces: replaces_id,
            icon: app_icon,
            summary,
            body,
            actions,
            hints,
            timeout: expire_timeout,
            id,
        });
        id
    }

    fn close_notification(&self, id: u32) {
        self.seen.lock().closed.push(id);
    }

    #[allow(clippy::unused_self)] // A method zbus serves.
    fn get_capabilities(&self) -> Vec<String> {
        ["actions", "body", "body-markup"]
            .map(String::from)
            .to_vec()
    }
}

/// logind's manager: an inhibitor is one end of a socket pair, the
/// other kept here to see it closed.
struct Login {
    seen: Arc<Mutex<Seen>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
#[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
impl Login {
    fn inhibit(
        &self,
        what: String,
        who: String,
        why: String,
        mode: String,
    ) -> zbus::zvariant::OwnedFd {
        let (kept, handed) = UnixStream::pair().unwrap();
        kept.set_nonblocking(true).unwrap();
        let mut seen = self.seen.lock();
        seen.inhibited.push([what, who, why, mode]);
        seen.held.push(kept);
        std::os::fd::OwnedFd::from(handed).into()
    }
}

/// The portal's inhibitor: each inhibition answered with a request served
/// beforehand ([`Request`]).
struct Portal {
    seen: Arc<Mutex<Seen>>,
}

#[zbus::interface(name = "org.freedesktop.portal.Inhibit")]
#[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
impl Portal {
    fn inhibit(
        &self,
        window: String,
        flags: u32,
        options: HashMap<String, OwnedValue>,
    ) -> OwnedObjectPath {
        let mut seen = self.seen.lock();
        seen.portal.push((
            window,
            flags,
            string(options.get("reason")),
            string(options.get("handle_token")),
        ));
        OwnedObjectPath::try_from(format!("{REQUESTS}/r{}", seen.portal.len())).unwrap()
    }
}

struct Request {
    path: String,
    seen: Arc<Mutex<Seen>>,
}

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl Request {
    fn close(&self) {
        self.seen.lock().requests_closed.push(self.path.clone());
    }
}

/// A tray's watcher, which a host registered with when `hosted`.
struct Watcher {
    registered: Arc<Mutex<Vec<String>>>,
    hosted: bool,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
#[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        self.registered.lock().push(service);
    }

    #[allow(clippy::unused_self)] // A method zbus serves.
    fn register_status_notifier_host(&self, service: String) {
        let _ = service;
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.hosted
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().clone()
    }

    #[zbus(property)]
    #[allow(clippy::unused_self)] // A property zbus serves.
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// A watcher on `address`, owning its name: its connection, and the items
/// registered with it.
fn watcher(address: &str, hosted: bool) -> (Connection, Arc<Mutex<Vec<String>>>) {
    let registered = Arc::new(Mutex::new(Vec::new()));
    let connection = zbus::blocking::connection::Builder::address(address)
        .unwrap()
        .name(WATCHER)
        .unwrap()
        .serve_at(
            "/StatusNotifierWatcher",
            Watcher {
                registered: Arc::clone(&registered),
                hosted,
            },
        )
        .unwrap()
        .build()
        .unwrap();
    (connection, registered)
}

/// The watcher's process ended: its name gone with it.
fn stop_watcher(connection: Connection) {
    connection.release_name(WATCHER).unwrap();
    drop(connection);
}

/// Whether the other end of `kept` is still held open.
fn open(kept: &UnixStream) -> bool {
    match (&*kept).read(&mut [0u8; 1]) {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Ok(0) => false,
        other => panic!("the inhibitor wrote: {other:?}"),
    }
}

/// The session's connection the backend is handed: one a test may replace
/// (the bus started again) or take away (no bus).
type Session = Arc<Mutex<Option<Connection>>>;

/// A desktop told on a daemon of its own, its servers stood in for.
struct Harness {
    /// First, so it ends before the daemon does.
    desktop: Option<Desktop>,
    setup: Setup,
    session: Session,
    asked: Arc<Mutex<Vec<Asked>>>,
    seen: Arc<Mutex<Seen>>,
    showing: Arc<Mutex<linux::Showing>>,
    /// The servers' connection, owning their names.
    server: Connection,
    /// A connection of the test's, as a host or a panel would ask.
    probe: Connection,
    daemon: Daemon,
}

impl Harness {
    fn desktop(&self) -> &Desktop {
        self.desktop.as_ref().unwrap()
    }

    fn notified(&self) -> usize {
        self.seen.lock().notified.len()
    }

    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().clone()
    }

    fn emit(&self, member: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
        self.server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                member,
                body,
            )
            .unwrap();
    }

    /// Call `method` of `interface` at `path` of `name`, as a host would.
    fn call<B>(
        &self,
        name: &str,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<zbus::Message>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.probe
            .call_method(Some(name), path, Some(interface), method, body)
    }

    fn has_owner(&self, name: &str) -> bool {
        self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            &name,
        )
        .unwrap()
        .body()
        .deserialize::<bool>()
        .unwrap()
    }

    /// The item's properties, as a host reads them.
    fn item(&self, name: &str) -> zbus::Result<HashMap<String, OwnedValue>> {
        self.call(
            name,
            "/StatusNotifierItem",
            "org.freedesktop.DBus.Properties",
            "GetAll",
            &"org.kde.StatusNotifierItem",
        )?
        .body()
        .deserialize()
    }

    fn item_call<B>(&self, name: &str, method: &str, body: &B)
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.call(
            name,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            method,
            body,
        )
        .unwrap();
    }

    /// The menu's layout: its revision, and each entry's id and
    /// properties.
    fn layout(&self, name: &str) -> (u32, Vec<(i32, HashMap<String, OwnedValue>)>) {
        let (revision, (id, _, entries)) = self
            .call(
                name,
                "/MenuBar",
                "com.canonical.dbusmenu",
                "GetLayout",
                &(0i32, -1i32, Vec::<String>::new()),
            )
            .unwrap()
            .body()
            .deserialize::<(u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>))>()
            .unwrap();
        assert_eq!(id, 0);
        let entries = entries
            .into_iter()
            .map(|entry| {
                let (id, properties, entries) =
                    <(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)>::try_from(Value::from(
                        entry,
                    ))
                    .unwrap();
                assert!(entries.is_empty(), "one level");
                (id, properties)
            })
            .collect();
        (revision, entries)
    }

    /// The menu's entry `id` clicked, as a host tells it.
    fn click(&self, name: &str, id: i32) {
        self.call(
            name,
            "/MenuBar",
            "com.canonical.dbusmenu",
            "Event",
            &(id, "clicked", Value::from(0i32), 0u32),
        )
        .unwrap();
    }

    /// The progresses told on the bus, from now: the entry's address,
    /// the progress and whether it shows.
    fn updates(&self) -> (Arc<Mutex<Vec<(String, f64, bool)>>>, CancelToken) {
        let told = Arc::new(Mutex::new(Vec::new()));
        let stop = CancelToken::new();
        let bus = connect(&self.daemon.address).unwrap();
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("com.canonical.Unity.LauncherEntry")
            .and_then(|rule| rule.member("Update"))
            .unwrap()
            .build();
        let (ready, listening) = crossbeam_channel::bounded(1);
        {
            let (told, stop) = (Arc::clone(&told), stop.clone());
            std::thread::spawn(move || {
                crate::bus::listen(
                    &bus,
                    vec![rule],
                    &stop,
                    move || ready.send(()).unwrap(),
                    |message| {
                        let (entry, properties) = message
                            .body()
                            .deserialize::<(String, HashMap<String, OwnedValue>)>()
                            .unwrap();
                        let progress = f64::try_from(&properties["progress"]).unwrap();
                        let visible = bool::try_from(&properties["progress-visible"]).unwrap();
                        told.lock().push((entry, progress, visible));
                    },
                );
            });
        }
        listening.recv_timeout(Duration::from_secs(10)).unwrap();
        (told, stop)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        drop(self.desktop.take());
    }
}

fn harness() -> Option<Harness> {
    let Some(daemon) = daemon() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return None;
    };
    let seen = Arc::new(Mutex::new(Seen::default()));
    let mut server = zbus::blocking::connection::Builder::address(daemon.address.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .name("org.freedesktop.login1")
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            Notifications {
                seen: Arc::clone(&seen),
                next: AtomicU32::new(41),
            },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/login1",
            Login {
                seen: Arc::clone(&seen),
            },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/portal/desktop",
            Portal {
                seen: Arc::clone(&seen),
            },
        )
        .unwrap();
    for request in 1..=4 {
        let path = format!("{REQUESTS}/r{request}");
        server = server
            .serve_at(
                path.clone(),
                Request {
                    path,
                    seen: Arc::clone(&seen),
                },
            )
            .unwrap();
    }
    let server = server.build().unwrap();
    let session: Session = Arc::new(Mutex::new(Some(connect(&daemon.address).unwrap())));
    let address = daemon.address.clone();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let showing = Arc::new(Mutex::new(linux::Showing::default()));
    let setup = Setup {
        app_id: "org.skwir.skwir".into(),
        app_name: "skwir".into(),
        icon: ICON,
        asked: {
            let asked = Arc::clone(&asked);
            Arc::new(move |what| asked.lock().push(what))
        },
        buses: linux::Buses {
            session: {
                let session = Arc::clone(&session);
                Arc::new(move || {
                    session
                        .lock()
                        .clone()
                        .ok_or_else(|| zbus::Error::Failure("no session bus".into()))
                })
            },
            system: Arc::new(move || connect(&address)),
        },
        showing: Arc::clone(&showing),
    };
    Some(Harness {
        desktop: Some(Desktop::with(setup.clone())),
        setup,
        session,
        asked,
        seen,
        showing,
        server,
        probe: connect(&daemon.address).unwrap(),
        daemon,
    })
}

fn notice(title: &str, body: &str, kind: Kind) -> Notice {
    Notice {
        title: title.into(),
        body: body.into(),
        kind,
    }
}

fn presence(standing: Standing, pause: Option<bool>) -> Presence {
    Presence {
        title: "Finishing 2 actions".into(),
        said: "Copy 3 items to Back & up · 42 % · about 3 min left".into(),
        about: "Copy 3 items to Back & up".into(),
        fraction: Some(0.42),
        standing,
        pause,
    }
}

fn string(value: Option<&OwnedValue>) -> String {
    String::try_from(value.unwrap().try_clone().unwrap()).unwrap()
}

/// A notice is shown as the specification asks, the next in its place,
/// and taken back by its key alone, once.
#[test]
fn notices_are_shown_in_one_place_and_taken_back() {
    let Some(harness) = harness() else { return };
    let desktop = harness.desktop();
    desktop.notify(1, notice("Copied", "Tom & Jerry's <files>", Kind::Finished));
    until("shown", &|| harness.notified() == 1);
    let first = {
        let seen = harness.seen.lock();
        let shown = &seen.notified[0];
        assert_eq!(shown.app_name, "skwir");
        assert_eq!(shown.replaces, 0);
        assert_eq!(shown.icon, "org.skwir.skwir");
        assert_eq!(shown.summary, "Copied");
        assert_eq!(
            shown.body, "Tom &amp; Jerry's &lt;files&gt;",
            "markup escaped"
        );
        assert_eq!(shown.actions, ["default", "Show"]);
        assert_eq!(string(shown.hint("category")), "transfer.complete");
        assert_eq!(string(shown.hint("desktop-entry")), "org.skwir.skwir");
        assert_eq!(u8::try_from(shown.hint("urgency").unwrap()).unwrap(), 1);
        assert_eq!(shown.timeout, -1);
        shown.id
    };

    desktop.notify(2, notice("Copy stopped", "No room", Kind::Stopped));
    until("shown in its place", &|| harness.notified() == 2);
    {
        let seen = harness.seen.lock();
        assert_eq!(seen.notified[1].replaces, first);
        assert_eq!(string(seen.notified[1].hint("category")), "transfer.error");
    }

    // Another key's: nothing taken back; its own: taken back.
    desktop.withdraw_key(1);
    desktop.withdraw_key(2);
    until("taken back", &|| !harness.seen.lock().closed.is_empty());
    // Nothing shown any more: nothing to take back, and the next is
    // shown anew. Asks are carried out in order: a close sent by
    // either would come before it.
    desktop.withdraw();
    desktop.notify(3, notice("Waiting", "Replace it?", Kind::Waiting));
    until("shown anew", &|| harness.notified() == 3);
    let seen = harness.seen.lock();
    assert_eq!(seen.closed, [first], "taken back once");
    assert_eq!(seen.notified[2].replaces, 0);
    assert_eq!(string(seen.notified[2].hint("category")), "transfer");
}

/// A click on the notice shown is told with its key and the token the
/// server gave just before; another program's notice is not ours.
#[test]
fn a_click_on_the_notice_shown_is_told_with_its_token() {
    let Some(harness) = harness() else { return };
    harness
        .desktop()
        .notify(7, notice("Moved", "12 files", Kind::Finished));
    // Known as shown before the server is heard: a signal before would
    // name no notice of ours.
    until("shown", &|| harness.showing.lock().shown.is_some());
    let id = harness.seen.lock().notified[0].id;

    harness.emit("ActivationToken", &(id, "token-7"));
    harness.emit("ActionInvoked", &(id, "default"));
    until("clicked", &|| harness.asked.lock().len() == 1);
    assert_eq!(
        harness.asked()[0],
        Asked::Notice(Clicked {
            key: 7,
            token: Some("token-7".into())
        })
    );

    // Another program's notice, then ours again, without a token:
    // signals come in order, so a click told for the other would come
    // first.
    harness.emit("ActivationToken", &(id + 100, "token-other"));
    harness.emit("ActionInvoked", &(id + 100, "default"));
    harness.emit("ActionInvoked", &(id, "default"));
    until("clicked again", &|| harness.asked.lock().len() == 2);
    assert_eq!(
        harness.asked()[1],
        Asked::Notice(Clicked {
            key: 7,
            token: None
        })
    );
}

/// A notice the server closed is forgotten: nothing taken back, and
/// the next shown anew.
#[test]
fn a_notice_closed_by_the_server_is_not_taken_back() {
    let Some(harness) = harness() else { return };
    harness
        .desktop()
        .notify(5, notice("Copied", "3 files", Kind::Finished));
    until("shown", &|| harness.showing.lock().shown.is_some());
    let id = harness.seen.lock().notified[0].id;
    // Another's closed first: ours stays.
    harness.emit("NotificationClosed", &(id + 100, 2u32));
    harness.emit("NotificationClosed", &(id, 2u32));
    until("forgotten", &|| harness.showing.lock().shown.is_none());

    harness.desktop().withdraw();
    harness
        .desktop()
        .notify(6, notice("Copied", "4 files", Kind::Finished));
    until("shown anew", &|| harness.notified() == 2);
    let seen = harness.seen.lock();
    assert!(seen.closed.is_empty(), "nothing taken back");
    assert_eq!(seen.notified[1].replaces, 0);
}

/// What another program says of our notices' ids is not ours; the server
/// gone, what it showed is forgotten, and the notice that stays is shown
/// anew by the one that came, whose clicks alone are ours.
#[test]
fn only_the_server_that_showed_ours_is_heard() {
    let Some(harness) = harness() else { return };
    // No tray: the presence is a notice that stays. Driven alone, each
    // ask carried out before the next.
    let mut backend = linux::Backend::new(&harness.setup);
    backend.presence(Some(&presence(Standing::Running, Some(true))));
    backend.notify(4, &notice("Copied", "3 files", Kind::Finished));
    let (resident, shown) = {
        let seen = harness.seen.lock();
        (seen.notified[0].id, seen.notified[1].id)
    };

    let other = connect(&harness.daemon.address).unwrap();
    for id in [shown, resident] {
        other
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "ActionInvoked",
                &(id, "default"),
            )
            .unwrap();
    }
    // Ours after it: what the bus heard first would be told first.
    harness.emit("ActionInvoked", &(shown, "default"));
    harness.emit("ActionInvoked", &(resident, "stop"));
    until("ours told", &|| harness.asked.lock().len() >= 2);
    assert_eq!(
        harness.asked(),
        [
            Asked::Notice(Clicked {
                key: 4,
                token: None
            }),
            Asked::StopAll
        ],
        "another program's not"
    );

    let before = harness.showing.lock().servers;
    harness
        .server
        .release_name("org.freedesktop.Notifications")
        .unwrap();
    let again = zbus::blocking::connection::Builder::address(harness.daemon.address.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            Notifications {
                seen: Arc::clone(&harness.seen),
                next: AtomicU32::new(1),
            },
        )
        .unwrap()
        .build()
        .unwrap();
    until("the server gone and come", &|| {
        harness.showing.lock().servers == before.wrapping_add(2)
    });
    {
        let showing = harness.showing.lock();
        assert_eq!((showing.shown, showing.resident), (None, None), "forgotten");
    }
    backend.woken();
    let anew = {
        let seen = harness.seen.lock();
        assert_eq!(seen.notified.len(), 3, "the notice that stays shown anew");
        assert_eq!(seen.notified[2].replaces, 0);
        seen.notified[2].id
    };

    // The last server's word on the new id is not the new server's.
    harness.emit("ActionInvoked", &(anew, "stop"));
    again
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(anew, "default"),
        )
        .unwrap();
    until("the new server's told", &|| harness.asked.lock().len() == 3);
    assert_eq!(harness.asked()[2], Asked::Open { token: None });
    drop(backend);
}

/// Progress is told on the launcher's entry at a pace, the last asked
/// told; what shows the same (a fraction of a percent, paused) is not
/// told again; an error hides it.
#[test]
fn progress_is_told_paced_on_the_launcher_entry() {
    let Some(harness) = harness() else { return };
    let (told, stop) = harness.updates();
    let progress = |fraction, state| Some(Progress { fraction, state });
    for step in 0..=100u8 {
        harness
            .desktop()
            .progress(progress(f32::from(step) / 200.0, State::Normal));
    }
    until("the last asked told", &|| {
        told.lock()
            .last()
            .is_some_and(|last| (last.1 - 0.5).abs() < 1e-9)
    });
    {
        let told = told.lock();
        assert!(told.len() <= 3, "paced: {told:?}");
        for (entry, _, visible) in told.iter() {
            assert_eq!(entry, "application://org.skwir.skwir.desktop");
            assert!(visible);
        }
    }
    let before = told.lock().len();
    harness.desktop().progress(progress(0.504, State::Normal));
    harness.desktop().progress(progress(0.5, State::Paused));
    harness.desktop().progress(progress(0.5, State::Error));
    until("hidden", &|| told.lock().last().is_some_and(|last| !last.2));
    assert_eq!(told.lock().len(), before + 1, "only the change told");
    stop.cancel();
}

/// Kept awake, logind is asked once and its descriptor held until let
/// go; dropped, the desktop lets go and hides the progress.
#[test]
fn kept_awake_holds_loginds_descriptor_until_let_go() {
    let Some(mut harness) = harness() else { return };
    let (told, stop) = harness.updates();
    let desktop = harness.desktop();
    desktop.keep_awake(Some("Copying 3 files".into()));
    desktop.keep_awake(Some("Moving".into()));
    // Asks are carried out in order: once this is told, both were.
    desktop.progress(Some(Progress {
        fraction: 0.25,
        state: State::Normal,
    }));
    until("told", &|| !told.lock().is_empty());
    {
        let seen = harness.seen.lock();
        assert_eq!(
            seen.inhibited,
            [["sleep", "skwir", "Copying 3 files", "block"].map(String::from)],
            "asked once, with the first why"
        );
        assert!(open(&seen.held[0]), "held");
    }

    desktop.keep_awake(None);
    until("let go", &|| !open(&harness.seen.lock().held[0]));
    desktop.keep_awake(Some("Copying again".into()));
    until("asked again", &|| harness.seen.lock().inhibited.len() == 2);
    assert_eq!(harness.seen.lock().inhibited[1][2], "Copying again");

    drop(harness.desktop.take());
    until("let go when dropped", &|| {
        let seen = harness.seen.lock();
        seen.held.len() == 2 && !open(&seen.held[1])
    });
    until("hidden when dropped", &|| {
        told.lock().last().is_some_and(|last| !last.2)
    });
    stop.cancel();
}

/// Nothing asked but what undoes: no thread, no bus touched.
#[test]
fn undoing_alone_starts_nothing() {
    let Some(harness) = harness() else { return };
    let desktop = harness.desktop();
    desktop.withdraw();
    desktop.withdraw_key(3);
    desktop.progress(None);
    desktop.keep_awake(None);
    desktop.presence(None);
    desktop.hold_session(None);
    assert!(desktop.started.get().is_none());
}

/// With a tray, the presence is an item registered with its watcher under
/// a name of its own, its properties as the specification has them; a
/// click after the host's token is told with it; its status follows how
/// it stands; taken back, its name and objects go.
#[test]
fn the_presence_is_an_item_in_the_tray() {
    let Some(harness) = harness() else { return };
    let (_watcher, registered) = watcher(&harness.daemon.address, true);
    harness
        .desktop()
        .presence(Some(presence(Standing::Running, Some(true))));
    until("registered", &|| registered.lock().len() == 1);
    let name = registered.lock()[0].clone();
    assert_eq!(
        name,
        format!("org.kde.StatusNotifierItem-{}-1", std::process::id())
    );

    let item = harness.item(&name).unwrap();
    let text = |key: &str| string(item.get(key));
    assert_eq!(text("Category"), "ApplicationStatus");
    assert_eq!(text("Id"), "org.skwir.skwir");
    assert_eq!(text("Title"), "skwir");
    assert_eq!(text("Status"), "Active");
    assert_eq!(text("IconName"), "");
    assert_eq!(i32::try_from(&item["WindowId"]).unwrap(), 0);
    assert!(!bool::try_from(&item["ItemIsMenu"]).unwrap());
    assert_eq!(
        OwnedObjectPath::try_from(item["Menu"].try_clone().unwrap())
            .unwrap()
            .as_str(),
        "/MenuBar"
    );
    let pixmaps =
        Vec::<(i32, i32, Vec<u8>)>::try_from(Value::from(item["IconPixmap"].try_clone().unwrap()))
            .unwrap();
    assert_eq!(
        pixmaps.iter().map(|(w, h, _)| (*w, *h)).collect::<Vec<_>>(),
        [(16, 16), (22, 22), (32, 32), (48, 48)]
    );
    for (side, _, argb) in &pixmaps {
        assert_eq!(argb.len(), (side * side * 4) as usize);
        // ARGB: the icon's opaque tile in the upper left.
        let corner = ((side / 4 * side + side / 4) * 4) as usize;
        assert_eq!(argb[corner], 0xff, "alpha first");
    }
    let (empty, icons, title, said) =
        <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(Value::from(
            item["ToolTip"].try_clone().unwrap(),
        ))
        .unwrap();
    assert_eq!((empty.as_str(), icons.len()), ("", 0));
    assert_eq!(title, "Finishing 2 actions");
    assert_eq!(
        said, "Copy 3 items to Back &amp; up · 42 % · about 3 min left",
        "read as markup"
    );
    assert_eq!(harness.notified(), 0, "no notice beside the tray");

    // The host's token, then a click: told with it, once.
    harness.item_call(&name, "ProvideXdgActivationToken", &"token-1");
    harness.item_call(&name, "Activate", &(0i32, 0i32));
    harness.item_call(&name, "SecondaryActivate", &(0i32, 0i32));
    until("clicked twice", &|| harness.asked.lock().len() == 2);
    assert_eq!(
        harness.asked(),
        [
            Asked::Open {
                token: Some("token-1".into())
            },
            Asked::Open { token: None }
        ]
    );

    harness
        .desktop()
        .presence(Some(presence(Standing::Stopped, Some(true))));
    until("needs attention", &|| {
        harness
            .item(&name)
            .is_ok_and(|item| string(item.get("Status")) == "NeedsAttention")
    });

    harness.desktop().presence(None);
    until("its name released", &|| !harness.has_owner(&name));
    let client = (harness.setup.buses.session)().unwrap();
    let unique = client.unique_name().unwrap().to_string();
    assert!(harness.item(&unique).is_err(), "its objects taken back");
}

/// The menu's entries, its pausing one following the presence, and what
/// each tells when clicked.
#[test]
fn the_menu_tells_its_choices_by_what_it_says() {
    let Some(harness) = harness() else { return };
    let (_watcher, registered) = watcher(&harness.daemon.address, true);
    harness
        .desktop()
        .presence(Some(presence(Standing::Running, Some(true))));
    until("registered", &|| registered.lock().len() == 1);
    let name = registered.lock()[0].clone();

    let label = |entries: &[(i32, HashMap<String, OwnedValue>)], id: i32| {
        let (_, properties) = entries.iter().find(|(of, _)| *of == id).unwrap();
        properties
            .get("label")
            .map(|label| string(Some(label)))
            .unwrap_or_default()
    };
    let visible = |entries: &[(i32, HashMap<String, OwnedValue>)], id: i32| {
        let (_, properties) = entries.iter().find(|(of, _)| *of == id).unwrap();
        properties
            .get("visible")
            .is_none_or(|visible| bool::try_from(visible).unwrap())
    };
    let (first, entries) = harness.layout(&name);
    assert_eq!(
        entries.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(label(&entries, 1), "Open Skwir");
    assert_eq!(string(entries[1].1.get("type")), "separator");
    assert_eq!(label(&entries, 3), "Pause all");
    assert!(visible(&entries, 3));
    assert_eq!(label(&entries, 4), "Stop all and quit");

    harness.click(&name, 3);
    until("paused", &|| {
        harness.asked().last() == Some(&Asked::PauseAll)
    });

    harness
        .desktop()
        .presence(Some(presence(Standing::Paused, Some(false))));
    until("the entry says Go on", &|| {
        label(&harness.layout(&name).1, 3) == "Go on"
    });
    assert!(harness.layout(&name).0 > first, "a new revision");
    harness.click(&name, 3);
    harness.click(&name, 4);
    harness.click(&name, 1);
    until("each told", &|| harness.asked.lock().len() == 4);
    assert_eq!(
        harness.asked()[1..],
        [Asked::GoOn, Asked::StopAll, Asked::Open { token: None }]
    );

    // Neither pausing nor going on: hidden, and clicked tells nothing.
    harness
        .desktop()
        .presence(Some(presence(Standing::Waiting, None)));
    until("hidden", &|| !visible(&harness.layout(&name).1, 3));
    harness.click(&name, 3);
    harness.click(&name, 4);
    until("told", &|| harness.asked.lock().len() == 5);
    assert_eq!(
        harness.asked()[4],
        Asked::StopAll,
        "the hidden one told nothing"
    );

    // One property, and a group's.
    let one = harness
        .call(
            &name,
            "/MenuBar",
            "com.canonical.dbusmenu",
            "GetProperty",
            &(4i32, "label"),
        )
        .unwrap()
        .body()
        .deserialize::<OwnedValue>()
        .unwrap();
    assert_eq!(string(Some(&one)), "Stop all and quit");
    let group = harness
        .call(
            &name,
            "/MenuBar",
            "com.canonical.dbusmenu",
            "GetGroupProperties",
            &(vec![1i32, 4], vec!["label"]),
        )
        .unwrap()
        .body()
        .deserialize::<Vec<(i32, HashMap<String, OwnedValue>)>>()
        .unwrap();
    assert_eq!(
        group
            .iter()
            .map(|(id, properties)| (*id, string(properties.get("label"))))
            .collect::<Vec<_>>(),
        [(1, "Open Skwir".into()), (4, "Stop all and quit".into())]
    );
}

/// A watcher started again (a panel restarted) gets the item again, under
/// the same name, and no notice shows meanwhile.
#[test]
fn a_watcher_started_again_gets_the_item_again() {
    let Some(harness) = harness() else { return };
    let (first, registered) = watcher(&harness.daemon.address, true);
    harness
        .desktop()
        .presence(Some(presence(Standing::Running, Some(true))));
    until("registered", &|| registered.lock().len() == 1);
    let name = registered.lock()[0].clone();

    stop_watcher(first);
    let (_second, again) = watcher(&harness.daemon.address, true);
    until("registered again", &|| again.lock().len() == 1);
    assert_eq!(again.lock()[0], name);
    assert!(harness.has_owner(&name));
    assert_eq!(harness.notified(), 0, "no notice meanwhile");
}

/// A tray gone for good leaves a notice that stays in the item's place;
/// one coming back takes it back.
#[test]
fn a_tray_gone_for_good_leaves_a_notice() {
    let Some(harness) = harness() else { return };
    let (first, registered) = watcher(&harness.daemon.address, true);
    harness
        .desktop()
        .presence(Some(presence(Standing::Running, Some(true))));
    until("registered", &|| registered.lock().len() == 1);
    let name = registered.lock()[0].clone();

    let gone = Instant::now();
    stop_watcher(first);
    until("a notice in its place", &|| harness.notified() == 1);
    assert!(
        gone.elapsed() >= Duration::from_secs(2),
        "waited for the tray to come back"
    );
    let id = {
        let seen = harness.seen.lock();
        assert!(bool::try_from(seen.notified[0].hint("resident").unwrap()).unwrap());
        seen.notified[0].id
    };
    until("the item taken back", &|| !harness.has_owner(&name));

    let (_second, again) = watcher(&harness.daemon.address, true);
    until("back in the tray", &|| again.lock().len() == 1);
    assert_ne!(again.lock()[0], name, "a new item");
    until("the notice taken back", &|| {
        harness.seen.lock().closed.contains(&id)
    });
}

/// The bus started again while a tray gone was waited for: the item served
/// on the last one is shown anew on this one, as a notice while no tray is
/// there, and nothing is waited for any more (a deadline left behind came
/// due at once, for ever: the module's thread never slept again).
#[test]
fn a_bus_started_again_while_the_tray_is_lost_waits_for_nothing() {
    let Some(harness) = harness() else { return };
    let (first, registered) = watcher(&harness.daemon.address, true);
    let mut backend = linux::Backend::new(&harness.setup);
    let running = presence(Standing::Running, Some(true));
    backend.presence(Some(&running));
    assert_eq!(registered.lock().len(), 1);

    stop_watcher(first);
    backend.woken();
    assert!(backend.due().is_some(), "the tray waited for");

    // Asked of a bus started again before its listener tells it.
    *harness.session.lock() = Some(connect(&harness.daemon.address).unwrap());
    backend.presence(Some(&Presence {
        said: "Copy 3 items to Back & up · 43 %".into(),
        ..running.clone()
    }));
    assert_eq!(harness.notified(), 1, "a notice in the item's place");
    assert_eq!(backend.due(), None, "nothing waited for");
    backend.woken();
    assert_eq!(backend.due(), None);
    assert_eq!(harness.notified(), 1, "not shown again");
    drop(backend);
}

/// The tray gone and then the bus: the deadline that came due without a
/// bus to look on is not due again; once the bus is back, the tray is
/// waited for anew, then the notice shown.
#[test]
fn a_tray_lost_without_a_bus_is_not_due_for_ever() {
    let Some(harness) = harness() else { return };
    let (first, registered) = watcher(&harness.daemon.address, true);
    let mut backend = linux::Backend::new(&harness.setup);
    backend.presence(Some(&presence(Standing::Running, Some(true))));
    assert_eq!(registered.lock().len(), 1);
    stop_watcher(first);
    backend.woken();
    let due = backend.due().expect("the tray waited for");

    let bus = harness.session.lock().take();
    backend.tick(due);
    assert_eq!(backend.due(), None, "not due again without a bus");

    *harness.session.lock() = bus;
    backend.woken();
    let due = backend.due().expect("waited for anew");
    backend.tick(due);
    assert_eq!(backend.due(), None);
    assert_eq!(harness.notified(), 1, "the notice in the item's place");
    drop(backend);
}

/// Without a tray (no watcher, or one no host reads), the presence is a
/// notice that stays, shown again only when it says something new, never
/// for a percent, nor after the user closed it until then; its actions
/// tell Open and Stop, beside the notices shown one at a time; taken
/// back, closed.
#[test]
fn without_a_tray_the_presence_is_a_notice_that_stays() {
    let Some(harness) = harness() else { return };
    // A watcher that no host reads is no tray.
    let (_watcher, registered) = watcher(&harness.daemon.address, false);
    // Driven alone, each ask carried out before the next.
    let mut backend = linux::Backend::new(&harness.setup);
    let running = presence(Standing::Running, Some(true));
    backend.presence(Some(&running));
    let id = {
        let seen = harness.seen.lock();
        assert_eq!(seen.notified.len(), 1);
        let shown = &seen.notified[0];
        assert_eq!(shown.replaces, 0);
        assert_eq!(shown.summary, "Finishing 2 actions");
        assert_eq!(shown.body, "Copy 3 items to Back &amp; up");
        assert_eq!(shown.actions, ["default", "Open Skwir", "stop", "Stop all"]);
        assert!(bool::try_from(shown.hint("resident").unwrap()).unwrap());
        assert_eq!(u8::try_from(shown.hint("urgency").unwrap()).unwrap(), 1);
        assert_eq!(string(shown.hint("category")), "transfer");
        assert_eq!(string(shown.hint("desktop-entry")), "org.skwir.skwir");
        assert_eq!(shown.timeout, 0, "it stays");
        shown.id
    };
    assert!(registered.lock().is_empty());

    // A percent, the words that change with it: not shown again.
    let further = Presence {
        said: "Copy 3 items to Back & up · 43 % · about 3 min left".into(),
        fraction: Some(0.43),
        ..running.clone()
    };
    backend.presence(Some(&further));
    assert_eq!(harness.notified(), 1, "not for a percent");
    // What it is about: shown in its place.
    let other = Presence {
        about: "Move 2 items to Archive".into(),
        ..further.clone()
    };
    backend.presence(Some(&other));
    assert_eq!(harness.notified(), 2);
    assert_eq!(harness.seen.lock().notified[1].replaces, id);

    // Its actions, with the token given before; a notice shown one at a
    // time beside it is told apart.
    harness.emit("ActivationToken", &(id, "token-r"));
    harness.emit("ActionInvoked", &(id, "default"));
    harness.emit("ActionInvoked", &(id, "stop"));
    until("told", &|| harness.asked.lock().len() == 2);
    backend.notify(9, &notice("Copied", "3 files", Kind::Finished));
    let single = harness.seen.lock().notified[2].id;
    assert_ne!(single, id);
    harness.emit("ActionInvoked", &(single, "default"));
    until("told the notice's click", &|| {
        harness.asked.lock().len() == 3
    });
    assert_eq!(
        harness.asked(),
        [
            Asked::Open {
                token: Some("token-r".into())
            },
            Asked::StopAll,
            Asked::Notice(Clicked {
                key: 9,
                token: None
            })
        ]
    );

    // Closed by the user: not shown again for new words; for a new
    // standing, shown anew.
    harness.emit("NotificationClosed", &(id, 2u32));
    until("forgotten", &|| harness.showing.lock().resident.is_none());
    backend.presence(Some(&Presence {
        said: "Move 2 items to Archive · 50 %".into(),
        ..other.clone()
    }));
    assert_eq!(harness.notified(), 3, "closed by the user: not again");
    backend.presence(Some(&Presence {
        standing: Standing::Paused,
        ..other.clone()
    }));
    let anew = {
        let seen = harness.seen.lock();
        assert_eq!(seen.notified.len(), 4);
        assert_eq!(seen.notified[3].replaces, 0, "shown anew");
        seen.notified[3].id
    };

    backend.presence(None);
    assert_eq!(harness.seen.lock().closed, [anew], "closed");
    drop(backend);
}

/// The session's end is held off by the portal (logout, with the reason)
/// and logind (shutdown, block) where no session manager shows its own;
/// another why while held is not told; let go, it stays held a moment, so
/// a queue's next action finds it held, then the portal's request is
/// closed and logind's descriptor too; held again, anew with its why.
#[test]
fn the_session_is_held_by_the_portal_and_logind() {
    let Some(harness) = harness() else { return };
    let desktop = harness.desktop();
    desktop.hold_session(Some("Copy 3 items to Backup".into()));
    until("held", &|| {
        let seen = harness.seen.lock();
        seen.portal.len() == 1 && seen.inhibited.len() == 1
    });
    {
        let seen = harness.seen.lock();
        let (window, flags, reason, token) = &seen.portal[0];
        assert_eq!((window.as_str(), *flags), ("", 1), "logout");
        assert_eq!(reason, "Copy 3 items to Backup");
        assert!(token.starts_with("skwir_session_"), "{token}");
        assert_eq!(
            seen.inhibited[0],
            ["shutdown", "skwir", "Copy 3 items to Backup", "block"].map(String::from)
        );
        assert!(open(&seen.held[0]));
    }

    // Another why while held, let go and held again at once: nothing
    // told; let go for good: closed a moment later.
    desktop.hold_session(Some("Move 2 items to Archive".into()));
    desktop.hold_session(None);
    desktop.hold_session(Some("Move 2 items to Archive".into()));
    let let_go = Instant::now();
    desktop.hold_session(None);
    until("let go", &|| harness.seen.lock().requests_closed.len() == 1);
    assert!(let_go.elapsed() >= LINGER, "a moment later");
    {
        let seen = harness.seen.lock();
        assert_eq!(seen.portal.len(), 1, "nothing asked anew");
        assert_eq!(seen.inhibited.len(), 1);
        assert_eq!(seen.requests_closed, [format!("{REQUESTS}/r1")]);
        assert!(!open(&seen.held[0]), "let go");
    }

    desktop.hold_session(Some("Move 2 items to Archive".into()));
    until("held anew", &|| harness.seen.lock().portal.len() == 2);
    assert_eq!(harness.seen.lock().portal[1].2, "Move 2 items to Archive");
    desktop.hold_session(None);
    until("let go again", &|| {
        harness.seen.lock().requests_closed.len() == 2
    });
    let seen = harness.seen.lock();
    assert_eq!(seen.requests_closed[1], format!("{REQUESTS}/r2"));
    assert!(!open(&seen.held[1]));
}

/// A session manager that shows its own dialog: the portal asked to hold
/// off the session's end and sleep, logind not (GNOME's session takes
/// logind's weak block for the portal's, and logind refuses the user's
/// own Suspend while a block is held); let go, the portal's requests are
/// closed.
#[test]
fn with_a_session_manager_logind_is_not_asked() {
    let Some(harness) = harness() else { return };
    let _manager = zbus::blocking::connection::Builder::address(harness.daemon.address.as_str())
        .unwrap()
        .name("org.gnome.SessionManager")
        .unwrap()
        .build()
        .unwrap();
    let desktop = harness.desktop();
    desktop.hold_session(Some("Copy 3 items to Backup".into()));
    desktop.keep_awake(Some("Copying".into()));
    until("both held", &|| harness.seen.lock().portal.len() == 2);
    {
        let seen = harness.seen.lock();
        let asked: Vec<_> = seen
            .portal
            .iter()
            .map(|(_, flags, reason, _)| (*flags, reason.as_str()))
            .collect();
        assert_eq!(asked, [(1, "Copy 3 items to Backup"), (4, "Copying")]);
        assert!(seen.inhibited.is_empty(), "logind not asked");
    }

    desktop.keep_awake(None);
    until("sleep let go", &|| {
        harness.seen.lock().requests_closed.len() == 1
    });
    assert_eq!(
        harness.seen.lock().requests_closed,
        [format!("{REQUESTS}/r2")]
    );
    desktop.hold_session(None);
    until("the session let go", &|| {
        harness.seen.lock().requests_closed.len() == 2
    });
    assert!(harness.seen.lock().inhibited.is_empty());
}

/// Finished, the desktop tells what was asked before returning (the last
/// notice), takes back the presence and lets go of the session.
#[test]
fn finishing_tells_the_last_notice_and_lets_go() {
    let Some(mut harness) = harness() else { return };
    let (_watcher, registered) = watcher(&harness.daemon.address, true);
    let desktop = harness.desktop.take().unwrap();
    desktop.presence(Some(presence(Standing::Running, Some(true))));
    desktop.hold_session(Some("Copy 3 items to Backup".into()));
    until("registered", &|| registered.lock().len() == 1);
    let name = registered.lock()[0].clone();

    desktop.notify(1, notice("Copied", "3 items", Kind::Finished));
    desktop.finish(Duration::from_secs(10));
    // Nothing waited for: done before it returned.
    assert_eq!(harness.notified(), 1, "the last notice told");
    assert!(!harness.has_owner(&name), "the item taken back");
    let seen = harness.seen.lock();
    assert_eq!(seen.requests_closed.len(), 1, "the portal let go");
    assert!(!open(&seen.held[0]), "logind let go");
}
