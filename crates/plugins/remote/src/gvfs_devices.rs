//! The devices GVfs offers (ADR-096): a phone over MTP, a camera over PTP,
//! an iPhone over AFC, plugged in, mounted or not. Each kind has a volume
//! monitor of GVfs's on the session bus, which GIO's programs ask what is
//! there (`org.gtk.Private.RemoteVolumeMonitor`): asked here the same, its
//! list read again each time one says something changed, or GVfs's daemon
//! that a mount was made or let go of. Nothing is mounted by being
//! plugged in: a device is mounted once opened. Linux only.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{Structure, Value};

use crate::address::Address;
use crate::connector::Offered;

/// GVfs's volume monitors of devices, by their names on the bus.
const MONITORS: [&str; 3] = [
    "org.gtk.vfs.MTPVolumeMonitor",
    "org.gtk.vfs.GPhoto2VolumeMonitor",
    "org.gtk.vfs.AfcVolumeMonitor",
];

/// Where each answers, and what it speaks.
const PATH: &str = "/org/gtk/Private/RemoteVolumeMonitor";
const INTERFACE: &str = "org.gtk.Private.RemoteVolumeMonitor";

/// GVfs's daemon's list of mounts, which says when one is made or let go of.
const TRACKER: &str = "org.gtk.vfs.MountTracker";

/// How long a monitor may take to answer (one started for the question
/// lists what is plugged in first).
const PATIENCE: Duration = Duration::from_secs(10);

/// Tell `told` what GVfs's monitors on the bus `env` names (the session's
/// where it names none) offer, then again at each change, until `cancel`
/// or the bus goes. Blocks: on a thread of its own.
///
/// # Errors
/// When the bus cannot be reached or listened to.
pub fn follow(
    env: &[(OsString, OsString)],
    told: &Arc<dyn Fn(Vec<Offered>) + Send + Sync>,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let bus = crate::bus::connect(env, PATIENCE)?;
    // Listened to before the first list: nothing said in between is missed.
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
    for rule in [
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(INTERFACE)?
            .build(),
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(TRACKER)?
            .build(),
        // A monitor started again (by another program) lists anew.
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns("org.gtk.vfs")?
            .build(),
    ] {
        dbus.add_match_rule(rule)?;
    }
    let told_of = MessageIterator::from(&bus);
    let (closing, signal) = (bus.clone(), cancel.subscribe());
    std::thread::Builder::new()
        .name("skwir-gvfs-devices-end".into())
        .spawn(move || {
            let _ = signal.receiver().recv();
            // What waits on it ends.
            let _ = closing.close();
        })?;
    told(list(&bus));
    for message in told_of {
        let Ok(message) = message else { break };
        if cancel.is_cancelled() {
            break;
        }
        let header = message.header();
        if header.message_type() != zbus::message::Type::Signal {
            continue;
        }
        let said = header.interface().map(|i| i.as_str().to_owned());
        let monitors = said.as_deref() == Some(INTERFACE);
        let tracker = said.as_deref() == Some(TRACKER);
        let restarted = header.member().is_some_and(|m| m == "NameOwnerChanged")
            && message
                .body()
                .deserialize::<(String, String, String)>()
                .is_ok_and(|(name, _, _)| MONITORS.contains(&name.as_str()));
        // A question a monitor asks while mounting (`MountOpAskPassword`)
        // changes nothing listed.
        let asking = header
            .member()
            .is_some_and(|m| m.as_str().starts_with("MountOp"));
        if (monitors && !asking) || tracker || restarted {
            told(list(&bus));
        }
    }
    Ok(())
}

/// What the monitors offer now: each one GVfs installs, asked in turn
/// (the bus starting it if it does not run, as GIO's programs do).
fn list(bus: &Connection) -> Vec<Offered> {
    let mut offered: Vec<Offered> = Vec::new();
    for monitor in MONITORS {
        let answer = bus.call_method(Some(monitor), PATH, Some(INTERFACE), "List", &());
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                tracing::debug!("GVfs's {monitor} lists nothing here: {error}");
                continue;
            }
        };
        let body = answer.body();
        let Ok(lists) = body.deserialize::<Structure>() else {
            tracing::debug!("GVfs's {monitor} answered a list Skwir does not read");
            continue;
        };
        for volume in volumes(&lists) {
            if !offered.iter().any(|o| o.address == volume.address) {
                offered.push(volume);
            }
        }
    }
    offered.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.address.host.cmp(&b.address.host))
    });
    offered
}

/// The devices of a monitor's answer (`List`: its drives, its volumes, its
/// mounts), each volume read by the place of its fields: its name (1), its
/// address (5, `activation_uri`), whether it mounts (6), its identifiers
/// (10, `unix-device`). One whose address names no device Skwir opens is
/// left out; fields GVfs adds after are not read.
pub fn volumes(lists: &Structure<'_>) -> Vec<Offered> {
    let Some(Value::Array(volumes)) = lists.fields().get(1) else {
        return Vec::new();
    };
    volumes
        .inner()
        .iter()
        .filter_map(|volume| {
            let Value::Structure(volume) = volume else {
                return None;
            };
            let fields = volume.fields();
            let text = |at: usize| match fields.get(at) {
                Some(Value::Str(text)) => Some(text.as_str()),
                _ => None,
            };
            let mounts = matches!(fields.get(6), Some(Value::Bool(true)));
            let address = Address::parse(text(5)?).ok()?;
            let device = address.scheme.device()?;
            let node = match fields.get(10) {
                Some(Value::Dict(ids)) => ids.iter().find_map(|(key, value)| match (key, value) {
                    (Value::Str(key), Value::Str(value)) if key.as_str() == "unix-device" => {
                        Some(PathBuf::from(value.as_str()))
                    }
                    _ => None,
                }),
                _ => None,
            };
            let name = text(1)
                .filter(|n| !n.trim().is_empty())
                .map_or_else(|| address.label(), |name| name.trim().to_owned());
            mounts.then_some(Offered {
                address: address.top(),
                name,
                device,
                node,
            })
        })
        .collect()
}
