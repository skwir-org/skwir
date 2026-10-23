//! Servers found on the local network (ADR-084): what the Nearby place
//! (`nearby:`) lists while a pane shows it, and only then. Found through
//! what the system runs for it: Avahi (DNS-SD, over the system bus: a NAS,
//! a Mac, a Linux server announcing SFTP, SMB, WebDAV, FTP, NFS or AFP)
//! and wsdd (WS-Discovery: Windows PCs), its daemon's socket where the
//! system runs one, else wsdd itself, started in discovery mode for as
//! long as the place is shown, as GVfs does. Skwir speaks no discovery
//! protocol: what goes on the network is what those send. Each server
//! opens by its address, as one typed does. Linux only.

// Elsewhere the browsers say "not yet": what only they use is unused
// there, and the place is not offered.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};
use skwir_core::location::{
    EmptyState, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use skwir_core::scheduler::Priority;

use crate::address::{Address, Scheme};

/// The scheme of the Nearby place.
pub const SCHEME: &str = "nearby";

/// The Nearby place.
pub fn location() -> Location {
    Location::parse("nearby:")
}

/// How long a listing waits for the browsers to say what they found
/// first before it is said complete: what comes later is added as it
/// comes.
const SETTLING: Duration = Duration::from_secs(3);

/// How often wsdd is asked what it found.
const WSDD_EVERY: Duration = Duration::from_secs(3);

/// The socket a system's wsdd daemon answers on, discovering (GVfs reads
/// it too).
const WSDD_SYSTEM: &str = "/run/wsdd.socket";

/// A server found: what it calls itself, what it speaks, where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub kind: &'static str,
    pub address: Address,
}

/// What a browser tells: a server seen (as `key`, an instance of it on an
/// interface and a protocol), one no longer, or that it said what it had.
#[derive(Debug)]
enum Seen {
    New(String, Found),
    Gone(String),
    Settled,
}

/// DNS-SD's service types Skwir opens: the scheme each is, and its name.
/// A host that offers ssh offers SFTP with it, mostly.
const TYPES: [(&str, Scheme, &str); 8] = [
    ("_sftp-ssh._tcp", Scheme::Sftp, "SFTP"),
    ("_ssh._tcp", Scheme::Sftp, "SFTP"),
    ("_smb._tcp", Scheme::Smb, "SMB"),
    ("_webdav._tcp", Scheme::Dav, "WebDAV"),
    ("_webdavs._tcp", Scheme::Davs, "WebDAV"),
    ("_ftp._tcp", Scheme::Ftp, "FTP"),
    ("_nfs._tcp", Scheme::Nfs, "NFS"),
    ("_afpovertcp._tcp", Scheme::Afp, "AFP"),
];

/// The address a service opens at: by its address (a `.local` name
/// resolves only where nss-mdns is installed), on its port, in the folder
/// its TXT names (`path=`, WebDAV's, FTP's, NFS's). `None` for an address
/// no connector reaches as written (an IPv6 one of the link alone).
pub fn address_of(scheme: Scheme, address: &str, port: u16, txt: &[Vec<u8>]) -> Option<Address> {
    let ip: std::net::IpAddr = address.parse().ok()?;
    let host = match ip {
        std::net::IpAddr::V4(v4) => v4.to_string(),
        std::net::IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => return None,
        std::net::IpAddr::V6(v6) => format!("[{v6}]"),
    };
    let path = txt
        .iter()
        .filter_map(|entry| entry.strip_prefix(b"path="))
        .find_map(|path| std::str::from_utf8(path).ok())
        .filter(|path| path.starts_with('/'))
        .unwrap_or("/");
    Address::parse(&format!("{}://{host}:{port}{path}", scheme.name())).ok()
}

/// The PCs a line of wsdd's `list` names: `urn:uuid:…`, the name, its
/// workgroup or domain, when last seen, the interface and its addresses
/// (`va, {'[fe80::1]', '10.0.0.2'}`), its types. Each is SMB, by its first
/// IPv4 address.
fn wsdd_found(line: &str) -> Option<(String, Found)> {
    let fields: Vec<&str> = line.split('\t').collect();
    let (id, name, at) = (fields.first()?, fields.get(1)?, fields.get(4)?);
    let (_, addresses) = at.split_once('{')?;
    let ip = addresses
        .trim_end_matches('}')
        .split(',')
        .map(|a| a.trim().trim_matches('\'').trim_matches(['[', ']']))
        .find(|a| a.parse::<std::net::Ipv4Addr>().is_ok())?;
    let address = Address::parse(&format!("smb://{ip}/")).ok()?;
    Some((
        format!("wsdd/{id}"),
        Found {
            name: (*name).to_owned(),
            kind: "SMB",
            address,
        },
    ))
}

/// The Nearby place's provider.
pub struct Provider;

impl LocationProvider for Provider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        Ok(Open::start(location.clone(), &ctx))
    }

    fn title(&self, _location: &Location) -> Option<String> {
        Some("Nearby".into())
    }

    fn empty_state(&self, _location: &Location) -> Option<EmptyState> {
        Some(EmptyState {
            title: "No server found nearby".into(),
            detail: "Servers that announce themselves on this network (a NAS, a Mac, a \
                   Windows PC) appear here, found by Avahi and wsdd while this place is \
                   shown."
                .into(),
        })
    }
}

/// The Nearby place, open: its browsers run until it closes.
struct Open {
    location: Location,
    events: crossbeam_channel::Receiver<ListingEvent>,
    cancel: CancelToken,
    /// Where each server listed leads.
    leads: Arc<Mutex<HashMap<EntryId, Location>>>,
}

impl Open {
    fn start(location: Location, ctx: &OpenContext) -> Arc<Self> {
        let (sink, events) = ctx.channel();
        let cancel = ctx.cancel.child();
        let leads: Arc<Mutex<HashMap<EntryId, Location>>> = Arc::default();
        let open = Arc::new(Open {
            location,
            events,
            cancel: cancel.clone(),
            leads: Arc::clone(&leads),
        });
        let started = std::thread::Builder::new()
            .name("skwir-nearby".into())
            .spawn(move || listing(&sink, &leads, &cancel));
        if let Err(error) = started {
            tracing::warn!("no thread to find servers nearby: {error}");
        }
        open
    }
}

impl OpenLocation for Open {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &crossbeam_channel::Receiver<ListingEvent> {
        &self.events
    }

    fn hydrate(&self, _ids: &[EntryId], _priority: Priority) {}

    fn path_of(&self, _id: EntryId) -> Option<PathBuf> {
        None
    }

    fn location_of(&self, id: EntryId) -> Option<Location> {
        self.leads.lock().get(&id).cloned()
    }

    /// The browsers tell what changes as it changes.
    fn refresh(&self) {}

    fn close(&self) {
        self.cancel.cancel();
    }
}

/// Let go of, it browses no more.
impl Drop for Open {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// A server's row: a place, named with what it speaks, leading to its
/// address.
fn entry_of(found: &Found) -> (EntryId, Entry) {
    let address = found.address.to_string();
    let id = EntryId::place(stable_hash(address.as_bytes()));
    let mut entry = Entry::new(
        id,
        format!("{} ({})", found.name, found.kind),
        EntryKind::Directory,
    );
    entry.metadata.set(
        keys::PLACE_KIND,
        Value::Text(format!("{} server", found.kind).into()),
        Provenance::Plugin {
            name: crate::ID.into(),
        },
    );
    (id, entry)
}

/// A 64-bit FNV-1a hash: a server's row is the same each time it is found.
fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Run the browsers until `cancel`, listing what they find: a server
/// once, however many of its instances (interfaces, protocols, its SFTP
/// announced twice) are seen, gone with the last.
fn listing(
    sink: &skwir_core::location::EventSink,
    leads: &Mutex<HashMap<EntryId, Location>>,
    cancel: &CancelToken,
) {
    let (tx, rx) = crossbeam_channel::unbounded::<Seen>();
    let mut browsers = 0;
    let mut failed = Vec::new();
    for (name, browse) in [
        (
            "Avahi",
            browse_avahi as fn(&dyn Fn(Seen), &CancelToken) -> anyhow::Result<()>,
        ),
        ("wsdd", browse_wsdd),
    ] {
        let (tx, cancel) = (tx.clone(), cancel.clone());
        let (said_tx, said) = crossbeam_channel::bounded(1);
        let started = std::thread::Builder::new()
            .name("skwir-nearby-browse".into())
            .spawn(move || {
                let seen = |s: Seen| {
                    let _ = tx.send(s);
                };
                if let Err(error) = browse(&seen, &cancel) {
                    let _ = said_tx.send(format!("{name}: {error:#}"));
                }
                // Done: whatever it said, it says nothing more.
                let _ = tx.send(Seen::Settled);
            });
        if started.is_ok() {
            browsers += 1;
            failed.push(said);
        }
    }
    drop(tx);
    let signal = cancel.subscribe();
    let settle = crossbeam_channel::after(SETTLING);
    let (mut settled, mut complete) = (0, false);
    // Each instance seen, and the server it is.
    let mut instances: HashMap<String, (EntryId, Found)> = HashMap::new();
    loop {
        let seen = crossbeam_channel::select! {
            recv(rx) -> seen => match seen {
                Ok(seen) => Some(seen),
                Err(_) => break,
            },
            recv(signal.receiver()) -> _ => break,
            recv(settle) -> _ => None,
        };
        match seen {
            Some(Seen::New(key, found)) => {
                let (id, entry) = entry_of(&found);
                let known = instances.values().any(|(other, _)| *other == id);
                leads.lock().insert(id, found.address.location());
                instances.insert(key, (id, found));
                if !known {
                    sink.send(ListingEvent::Added(vec![entry]));
                }
            }
            Some(Seen::Gone(key)) => {
                let Some((id, _)) = instances.remove(&key) else {
                    continue;
                };
                if !instances.values().any(|(other, _)| *other == id) {
                    leads.lock().remove(&id);
                    sink.send(ListingEvent::Removed(vec![id]));
                }
            }
            Some(Seen::Settled) => settled += 1,
            None => settled = browsers,
        }
        if settled >= browsers && !complete {
            complete = true;
            let said: Vec<String> = failed.iter().filter_map(|f| f.try_recv().ok()).collect();
            if said.len() == browsers && instances.is_empty() {
                sink.send(ListingEvent::Error(format!(
                    "Servers nearby are found by Avahi or wsdd, and neither answers here ({})",
                    said.join("; ")
                )));
            }
            sink.send(ListingEvent::Complete);
        }
    }
}

/// What Avahi finds of [`TYPES`] on the system bus, each resolved as it
/// comes, until `cancel`: then its browsers are freed (nothing more is
/// asked of the network) and the connection closed.
#[cfg(target_os = "linux")]
fn browse_avahi(seen: &dyn Fn(Seen), cancel: &CancelToken) -> anyhow::Result<()> {
    use zbus::blocking::{Connection, MessageIterator, Proxy};
    use zbus::zvariant::OwnedObjectPath;
    const AVAHI: &str = "org.freedesktop.Avahi";
    const BROWSER: &str = "org.freedesktop.Avahi.ServiceBrowser";
    /// What `ResolveService` answers: the service, its host, its address
    /// and port, its TXT.
    type Resolved = (
        i32,
        i32,
        String,
        String,
        String,
        String,
        i32,
        String,
        u16,
        Vec<Vec<u8>>,
        u32,
    );
    let bus = Connection::system()?;
    // Listened to before any browser starts: nothing it tells is missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(AVAHI)?
        .interface(BROWSER)?
        .build();
    let told = MessageIterator::for_match_rule(rule, &bus, None)?;
    let server = Proxy::new(&bus, AVAHI, "/", "org.freedesktop.Avahi.Server")?;
    let mut browsers: Vec<(OwnedObjectPath, Scheme, &'static str)> = Vec::new();
    for (kind, scheme, name) in TYPES {
        let asked = (-1i32, -1i32, kind, "", 0u32);
        // Avahi 0.8 prepares a browser, started once it is listened to;
        // an older one starts it at once.
        let path: OwnedObjectPath = match server.call("ServiceBrowserPrepare", &asked) {
            Ok(path) => {
                let browser: OwnedObjectPath = path;
                Proxy::new(&bus, AVAHI, browser.as_ref(), BROWSER)?
                    .call::<_, _, ()>("Start", &())?;
                browser
            }
            Err(_) => server.call("ServiceBrowserNew", &asked)?,
        };
        browsers.push((path, scheme, name));
    }
    let freed: Vec<OwnedObjectPath> = browsers.iter().map(|(p, ..)| p.clone()).collect();
    let (closing, signal) = (bus.clone(), cancel.subscribe());
    std::thread::Builder::new()
        .name("skwir-nearby-avahi".into())
        .spawn(move || {
            let _ = signal.receiver().recv();
            for path in freed {
                let _ = Proxy::new(&closing, AVAHI, path.as_ref(), BROWSER)
                    .and_then(|browser| browser.call::<_, _, ()>("Free", &()));
            }
            // What waits on it ends.
            let _ = closing.close();
        })?;
    let mut settled = HashSet::new();
    for message in told {
        let Ok(message) = message else { break };
        if cancel.is_cancelled() {
            break;
        }
        let header = message.header();
        let (Some(path), Some(member)) = (header.path(), header.member()) else {
            continue;
        };
        let Some((_, scheme, name)) = browsers.iter().find(|(p, ..)| p.as_str() == path.as_str())
        else {
            continue;
        };
        match member.as_str() {
            "ItemNew" | "ItemRemove" => {
                let (interface, protocol, service, kind, domain, _): (
                    i32,
                    i32,
                    String,
                    String,
                    String,
                    u32,
                ) = message.body().deserialize()?;
                let key = format!("avahi/{interface}/{protocol}/{service}/{kind}/{domain}");
                if member.as_str() == "ItemRemove" {
                    seen(Seen::Gone(key));
                    continue;
                }
                // Its IPv4 address first: one of the link alone opens
                // nowhere as written.
                let resolved = [0i32, -1].into_iter().find_map(|wanted| {
                    server
                        .call::<_, _, Resolved>(
                            "ResolveService",
                            &(interface, protocol, &service, &kind, &domain, wanted, 0u32),
                        )
                        .ok()
                });
                let Some((.., address, port, txt, _)) = resolved else {
                    continue;
                };
                if let Some(address) = address_of(*scheme, &address, port, &txt) {
                    seen(Seen::New(
                        key,
                        Found {
                            name: service,
                            kind: name,
                            address,
                        },
                    ));
                }
            }
            "AllForNow" | "Failure" => {
                settled.insert(path.to_string());
                if settled.len() == browsers.len() {
                    seen(Seen::Settled);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn browse_avahi(_seen: &dyn Fn(Seen), _cancel: &CancelToken) -> anyhow::Result<()> {
    Err(skwir_core::system::not_yet("Finding servers nearby").into())
}

/// The PCs wsdd finds, asked every [`WSDD_EVERY`] until `cancel`: the
/// system's daemon where it answers ([`WSDD_SYSTEM`]), else wsdd started
/// here in discovery mode, stopped with the place.
#[cfg(target_os = "linux")]
fn browse_wsdd(seen: &dyn Fn(Seen), cancel: &CancelToken) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};
    let system = Path::new(WSDD_SYSTEM);
    let (socket, _ours) = if system.exists() {
        (system.to_path_buf(), None)
    } else {
        let ours = OwnWsdd::start(cancel)?;
        (ours.socket.clone(), Some(ours))
    };
    let mut known: HashSet<String> = HashSet::new();
    let signal = cancel.subscribe();
    let mut first = true;
    loop {
        let mut stream = std::os::unix::net::UnixStream::connect(&socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(b"list\n")?;
        let mut now: HashMap<String, Found> = HashMap::new();
        for line in std::io::BufReader::new(stream).lines() {
            let line = line?;
            if line == "." {
                break;
            }
            if let Some((key, found)) = wsdd_found(&line) {
                now.insert(key, found);
            }
        }
        for key in known
            .difference(&now.keys().cloned().collect())
            .cloned()
            .collect::<Vec<_>>()
        {
            known.remove(&key);
            seen(Seen::Gone(key));
        }
        for (key, found) in now {
            if known.insert(key.clone()) {
                seen(Seen::New(key, found));
            }
        }
        if std::mem::take(&mut first) {
            seen(Seen::Settled);
        }
        if signal.receiver().recv_timeout(WSDD_EVERY).is_ok() {
            return Ok(());
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn browse_wsdd(_seen: &dyn Fn(Seen), _cancel: &CancelToken) -> anyhow::Result<()> {
    Err(skwir_core::system::not_yet("Finding servers nearby").into())
}

/// wsdd started here in discovery mode (`--no-host`: it announces
/// nothing), answering on a socket in the runtime folder; stopped once
/// dropped.
#[cfg(target_os = "linux")]
struct OwnWsdd {
    child: std::process::Child,
    socket: PathBuf,
}

#[cfg(target_os = "linux")]
impl OwnWsdd {
    fn start(cancel: &CancelToken) -> anyhow::Result<Self> {
        let program = skwir_core::os_path::find_program("wsdd")
            .ok_or_else(|| anyhow::anyhow!("wsdd is not installed"))?;
        let folder = skwir_core::xdg::runtime_dir()
            .ok_or_else(|| anyhow::anyhow!("no runtime folder"))?
            .join("skwir");
        std::fs::create_dir_all(&folder)?;
        let socket = folder.join(format!(
            "wsdd-{}-{}.sock",
            std::process::id(),
            stable_hash(format!("{:?}", std::time::Instant::now()).as_bytes()) % 100_000
        ));
        let child = std::process::Command::new(program)
            .args(["--no-host", "--discovery", "--listen"])
            .arg(&socket)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let own = OwnWsdd { child, socket };
        // It makes its socket once started: a moment.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !own.socket.exists() {
            anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
            anyhow::ensure!(std::time::Instant::now() < deadline, "wsdd did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(own)
    }
}

#[cfg(target_os = "linux")]
impl Drop for OwnWsdd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A service is opened by its address, on its port, in the folder its
    /// TXT names; an IPv6 address of the link alone opens nowhere.
    #[test]
    fn a_service_opens_at_its_address() {
        let txt = vec![b"u=me".to_vec(), b"path=/dav".to_vec()];
        let at = |scheme, address, port, txt: &[Vec<u8>]| {
            address_of(scheme, address, port, txt).map(|a| a.to_string())
        };
        assert_eq!(
            at(Scheme::Dav, "10.89.0.1", 8080, &txt).as_deref(),
            Some("dav://10.89.0.1:8080/dav")
        );
        assert_eq!(
            at(Scheme::Smb, "192.168.1.20", 445, &[]).as_deref(),
            Some("smb://192.168.1.20:445/")
        );
        assert_eq!(at(Scheme::Sftp, "fe80::1", 22, &[]), None);
        assert_eq!(
            at(Scheme::Sftp, "fd00::5", 22, &[]).as_deref(),
            Some("sftp://[fd00::5]:22/")
        );
        assert_eq!(at(Scheme::Sftp, "nas.local", 22, &[]), None);
    }

    /// A line of wsdd's list names a PC: SMB, by its IPv4 address.
    #[test]
    fn a_pc_wsdd_found_opens_over_smb() {
        let line = "urn:uuid:6a3e4c1e-0000-4000-8000-000000000001\tWINPC\tWorkgroup:TESTGROUP\t2026-10-23T02:11:35\tva, {'[fe80::64a0:3ff:fec2:911a]', '10.89.0.2'}\tpub:Computer";
        let (key, found) = wsdd_found(line).unwrap();
        assert_eq!(key, "wsdd/urn:uuid:6a3e4c1e-0000-4000-8000-000000000001");
        assert_eq!(found.name, "WINPC");
        assert_eq!(found.address.to_string(), "smb://10.89.0.2/");
        assert!(
            wsdd_found("urn:uuid:x\tPC\tWorkgroup:W\tt\tva, {'[fe80::1]'}\tpub:Computer").is_none()
        );
        assert!(wsdd_found(".").is_none());
    }
}
