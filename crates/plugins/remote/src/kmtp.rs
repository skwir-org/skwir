//! KDE's kmtpd (ADR-097): a phone KDE holds, which GVfs cannot open (one
//! program at a time uses a phone over USB, and kmtpd keeps every one it
//! sees), mounted by Skwir's helper (`skwir-kmtp-worker`) that serves
//! kmtpd's interface on the session bus as a folder. Asked only of a
//! `kiod6` that runs, and of a kmtpd already loaded, except on KDE's
//! desktop, where it is loaded as Dolphin would: elsewhere, loading it
//! would take every phone from GVfs. A phone kmtpd holds keeps the address
//! GVfs gives it (`mtp://<ID_SERIAL>/`, ADR-096). Linux only.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Offered, Recognised};
use crate::mounter;

/// The helper's mounts in the system's table.
pub const FSTYPE: &str = "fuse.skwir-kmtp";

/// The helper, where Skwir's helpers are.
pub const WORKER: skwir_platform::process::Worker = skwir_platform::process::Worker {
    variable: "SKWIR_KMTP_WORKER",
    executable: "skwir-kmtp-worker",
};

/// `kiod6`, which hosts kmtpd, and kmtpd's objects.
#[cfg(target_os = "linux")]
const SERVICE: &str = "org.kde.kiod6";
#[cfg(target_os = "linux")]
const MODULES: &str = "/modules";
#[cfg(target_os = "linux")]
const DAEMON_PATH: &str = "/modules/kmtpd";
#[cfg(target_os = "linux")]
const DAEMON: &str = "org.kde.kmtp.Daemon";
#[cfg(target_os = "linux")]
const DEVICE: &str = "org.kde.kmtp.Device";

/// How long mounting may take: kmtpd answers from what it holds.
#[cfg(target_os = "linux")]
const PATIENCE: Duration = Duration::from_secs(60);

/// How long a question to kmtpd may take, its storages' descriptions and
/// its devices asked of what it holds.
#[cfg(target_os = "linux")]
const ASKED: Duration = Duration::from_secs(10);

/// KDE's kmtpd, as the plugin asks it.
#[derive(Default)]
pub struct Kmtp {
    /// Variables the helper and the bus are reached with beside skwir's
    /// own: a test's private session bus.
    pub env: Vec<(OsString, OsString)>,
    /// The helper, where not found as Skwir's are (a test's build).
    pub helper: Option<PathBuf>,
    /// Whether this is KDE's desktop, where kmtpd may be loaded; `None`
    /// as `XDG_CURRENT_DESKTOP` says.
    pub on_kde: Option<bool>,
    /// Where sysfs and udev's data are, where not the system's (a test's
    /// stand-ins): a device's serial is read there.
    pub system: Option<PathBuf>,
}

/// A device kmtpd holds: how kmtpd knows it, and as what it is offered.
#[cfg(target_os = "linux")]
struct Held {
    udi: String,
    offered: Offered,
}

impl Kmtp {
    fn helper(&self) -> Option<PathBuf> {
        self.helper.clone().or_else(|| WORKER.find())
    }
}

#[cfg(target_os = "linux")]
impl Kmtp {
    fn on_kde(&self) -> bool {
        self.on_kde.unwrap_or_else(|| {
            skwir_platform::apps::current_desktops()
                .iter()
                .any(|desktop| desktop.eq_ignore_ascii_case("KDE"))
        })
    }

    /// The devices kmtpd holds, asked only where `kiod6` runs and kmtpd
    /// is loaded, unless `may_load`. Blocks: on a worker.
    fn held(&self, may_load: bool) -> anyhow::Result<Vec<Held>> {
        let bus = crate::bus::connect(&self.env, ASKED)?;
        if !may_load && !loaded(&bus)? {
            return Ok(Vec::new());
        }
        let daemon = proxy(&bus, DAEMON_PATH, DAEMON)?;
        let devices: Vec<zbus::zvariant::OwnedObjectPath> = daemon.call("listDevices", &())?;
        let mut held = Vec::new();
        for device in devices {
            let device = proxy(&bus, device.as_str(), DEVICE)?;
            let udi: String = device.get_property("udi")?;
            let name: String = device.get_property("friendlyName")?;
            let Some((address, node)) = self.of_udi(&udi) else {
                continue;
            };
            held.push(Held {
                udi,
                offered: Offered {
                    name: if name.trim().is_empty() {
                        address.label()
                    } else {
                        name.trim().to_owned()
                    },
                    address,
                    device: crate::connector::Device::Phone,
                    node,
                },
            });
        }
        Ok(held)
    }

    /// The address and node of the device Solid knows by `udi`
    /// (`/org/kde/solid/udev/sys/devices/…/9-1`): its folder of sysfs,
    /// udev's record of it by its numbers, the serial GVfs names it by.
    fn of_udi(&self, udi: &str) -> Option<(Address, Option<PathBuf>)> {
        let system = self.system.clone().unwrap_or_else(|| PathBuf::from("/"));
        let folder = udi.strip_prefix("/org/kde/solid/udev/")?;
        if folder.split('/').any(|part| part == "..") {
            return None;
        }
        let folder = system.join(folder);
        let numbers = std::fs::read_to_string(folder.join("dev")).ok()?;
        let record =
            std::fs::read_to_string(system.join(format!("run/udev/data/c{}", numbers.trim())))
                .ok()?;
        let serial = record
            .lines()
            .find_map(|line| line.strip_prefix("E:ID_SERIAL="))?;
        let address = Address::parse(&format!("mtp://{serial}/")).ok()?;
        let number = |name: &str| -> Option<u32> {
            std::fs::read_to_string(folder.join(name))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        let node = number("busnum")
            .zip(number("devnum"))
            .map(|(bus, dev)| PathBuf::from(format!("/dev/bus/usb/{bus:03}/{dev:03}")));
        Some((address, node))
    }

    /// The cache folder the helper keeps what it reads and writes in,
    /// unnamed.
    fn cache() -> PathBuf {
        skwir_core::cache::Cache::new(
            skwir_core::cache::Cache::default_root().join(skwir_core::cache::LAYOUT.to_string()),
        )
        .namespace(crate::ID)
        .folder("kmtp")
    }

    /// Mount the phone kmtpd holds at `address` through the helper.
    fn mount(&self, address: &Address, cancel: &CancelToken) -> anyhow::Result<Connected> {
        let top = address.top();
        let held = self
            .held(self.on_kde())?
            .into_iter()
            .find(|h| h.offered.address.same_server(&top))
            .ok_or_else(|| anyhow::anyhow!("KDE's kmtpd does not hold this phone"))?;
        let helper = self
            .helper()
            .ok_or_else(|| anyhow::anyhow!("{} is not installed", WORKER.executable))?;
        let point = mounter::point_for(self.id(), &top)?;
        let mut command = std::process::Command::new(helper);
        command
            .arg("mount")
            .arg(&held.udi)
            .arg(&point)
            .arg(Self::cache())
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        mounter::mount(&mut command, &point, cancel, PATIENCE)?;
        Ok(Connected {
            point,
            address: top,
        })
    }
}

/// Whether `kiod6` runs on `bus` with kmtpd loaded, asked without starting
/// either: the bus owner of the name, then what `/modules` holds, which
/// `kiod6` answers without loading anything.
#[cfg(target_os = "linux")]
fn loaded(bus: &zbus::blocking::Connection) -> anyhow::Result<bool> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus)?;
    if !dbus.name_has_owner(zbus::names::BusName::try_from(SERVICE)?)? {
        return Ok(false);
    }
    let modules = zbus::blocking::fdo::IntrospectableProxy::builder(bus)
        .destination(SERVICE)?
        .path(MODULES)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()?;
    Ok(modules.introspect()?.contains("<node name=\"kmtpd\""))
}

/// kmtpd's object at `path` speaking `interface`, its properties asked
/// one at a time.
#[cfg(target_os = "linux")]
fn proxy<'a>(
    bus: &'a zbus::blocking::Connection,
    path: &'a str,
    interface: &'static str,
) -> zbus::Result<zbus::blocking::Proxy<'a>> {
    zbus::blocking::proxy::Builder::<zbus::blocking::Proxy<'a>>::new(bus)
        .destination(SERVICE)?
        .path(path)?
        .interface(interface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
}

impl Connector for Kmtp {
    fn id(&self) -> &'static str {
        "kmtp"
    }

    fn title(&self) -> &'static str {
        "KDE's kmtpd"
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[Scheme::Mtp]
    }

    fn unavailable(&self) -> Option<String> {
        if !cfg!(target_os = "linux") {
            return Some("KDE's kmtpd is reached on Linux only".to_owned());
        }
        if self.helper().is_none() {
            return Some(format!("{} is not installed", WORKER.executable));
        }
        None
    }

    /// Only a phone kmtpd holds: any other goes to the next tool unsaid.
    fn opens(&self, address: &Address) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.held(self.on_kde()).is_ok_and(|held| {
                held.iter()
                    .any(|h| h.offered.address.same_server(&address.top()))
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = address;
            false
        }
    }

    fn connect(
        &self,
        address: &Address,
        _ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        #[cfg(target_os = "linux")]
        {
            self.mount(address, cancel)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (address, cancel);
            anyhow::bail!("KDE's kmtpd is reached on Linux only")
        }
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        if remote.fstype != FSTYPE {
            return None;
        }
        let mut ours = mounter::ours(self.id(), &remote.point)?;
        ours.device = ours.address.as_ref().and_then(|a| a.scheme.device());
        Some(ours)
    }

    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        mounter::unmount(&remote.point, recognised.ours, cancel)
    }

    /// A file is fetched whole before any of it is read: kmtpd reads no
    /// part of one.
    fn reads_whole(&self, _remote: &Remote) -> bool {
        true
    }

    /// The phones kmtpd holds, while `kiod6` runs and kmtpd is loaded (on
    /// KDE's desktop, loading it): asked again when kmtpd says its devices
    /// changed, or `kiod6` came or went.
    fn follow_devices(
        &self,
        told: Arc<dyn Fn(Vec<Offered>) + Send + Sync>,
        cancel: &CancelToken,
    ) -> bool {
        #[cfg(target_os = "linux")]
        {
            if self.unavailable().is_some() {
                return false;
            }
            let this = Kmtp {
                env: self.env.clone(),
                helper: self.helper.clone(),
                on_kde: Some(self.on_kde()),
                system: self.system.clone(),
            };
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("skwir-kmtpd-devices".into())
                .spawn(move || {
                    if let Err(error) = this.follow(&told, &cancel) {
                        tracing::debug!("the phones KDE holds are not followed: {error:#}");
                    }
                })
                .is_ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (told, cancel);
            false
        }
    }
}

impl Kmtp {
    /// [`Connector::follow_devices`] on this thread, until `cancel`.
    #[cfg(target_os = "linux")]
    fn follow(
        &self,
        told: &Arc<dyn Fn(Vec<Offered>) + Send + Sync>,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        let bus = crate::bus::connect(&self.env, ASKED)?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
        for rule in [
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg(0, SERVICE)?
                .build(),
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(DAEMON)?
                .member("devicesChanged")?
                .build(),
        ] {
            dbus.add_match_rule(rule)?;
        }
        let told_of = zbus::blocking::MessageIterator::from(&bus);
        let (closing, signal) = (bus.clone(), cancel.subscribe());
        std::thread::Builder::new()
            .name("skwir-kmtpd-end".into())
            .spawn(move || {
                let _ = signal.receiver().recv();
                let _ = closing.close();
            })?;
        let offered = |this: &Kmtp| {
            this.held(this.on_kde())
                .map(|held| held.into_iter().map(|h| h.offered).collect())
                .unwrap_or_default()
        };
        told(offered(self));
        for message in told_of {
            let Ok(message) = message else { break };
            if cancel.is_cancelled() {
                break;
            }
            let header = message.header();
            if header.message_type() != zbus::message::Type::Signal {
                continue;
            }
            if header
                .member()
                .is_some_and(|m| m == "NameOwnerChanged" || m == "devicesChanged")
            {
                told(offered(self));
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// A device is known by the serial udev gives it, read from its folder
    /// of sysfs, as GVfs names it: one address whichever holds it.
    #[test]
    fn a_device_is_known_by_its_serial() {
        let system = tempfile::tempdir().unwrap();
        let folder = system.path().join("sys/devices/pci0000:00/usb9/9-1");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("dev"), "189:1037\n").unwrap();
        std::fs::write(folder.join("busnum"), "9\n").unwrap();
        std::fs::write(folder.join("devnum"), "14\n").unwrap();
        let udev = system.path().join("run/udev/data");
        std::fs::create_dir_all(&udev).unwrap();
        std::fs::write(
            udev.join("c189:1037"),
            "I:1\nE:ID_MTP_DEVICE=1\nE:ID_SERIAL=Example_Phone_0123\n",
        )
        .unwrap();
        let kmtp = Kmtp {
            system: Some(system.path().to_path_buf()),
            ..Kmtp::default()
        };
        let (address, node) = kmtp
            .of_udi("/org/kde/solid/udev/sys/devices/pci0000:00/usb9/9-1")
            .unwrap();
        assert_eq!(address.to_string(), "mtp://Example_Phone_0123/");
        assert_eq!(node, Some(PathBuf::from("/dev/bus/usb/009/014")));
        assert!(
            kmtp.of_udi("/org/kde/solid/udev/sys/devices/gone")
                .is_none()
        );
        assert!(kmtp.of_udi("/not/solid").is_none());
        assert!(
            kmtp.of_udi("/org/kde/solid/udev/sys/../etc").is_none(),
            "never outside sysfs"
        );
    }
}
