//! Remote folders (ADR-077): an address (`sftp://`, `smb://`, `dav://`,
//! `ftp://`, `nfs://`, `afp://`, `rclone://`) opens by connecting it, through
//! an installed tool that mounts it, then showing the mounted folder. A
//! remote folder is a folder: everything works there. This plugin knows the
//! tools (connectors), names each mount in the sidebar's Network section,
//! lets go of them, and says what is shown of each unasked (thumbnails).
//! A phone or a camera GVfs reads is a device of this machine with an
//! address of its own (`mtp://`, `gphoto2://`, `afc://`): listed among the
//! sidebar's devices once plugged in, mounted once opened (ADR-096).
//!
//! Settings: `remote.connectors` (the tools, in the order tried),
//! `remote.thumbnails` (for a remote the user chose nothing for),
//! `remote.chosen` (what the user chose for each, by address).

// Tests run from a console: no console window to keep away.
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod address;
#[cfg(target_os = "linux")]
mod bus;
mod configure;
pub mod connector;
mod following;
pub mod fstab;
pub mod gio_copy;
pub mod gvfs;
mod gvfs_catalog;
#[cfg(target_os = "linux")]
pub mod gvfs_devices;
mod gvfs_spec;
mod holders;
pub mod keys;
mod keys_form;
pub mod kio;
pub mod kio_copy;
pub mod kmtp;
mod known;
pub mod lan;
pub mod login;
mod mounter;
pub mod nearby;
pub mod options;
pub mod passwords;
mod provider;
pub mod rc;
pub mod rclone;
pub mod rclone_config;
#[cfg(unix)]
pub mod rclone_copy;
pub mod rclone_features;
pub mod rclone_forms;
pub mod rsync;
mod runs;
pub mod servers;
pub mod servers_form;
pub mod sftp;
mod sources;
mod ssh_mount;
pub mod sshfs;
pub mod system_shares;
pub mod uploads;
pub mod winfsp;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::fs::reach::{self, Known, Remote, Thumbnails};
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::Place;
use skwir_core::scheduler::Priority;
use skwir_core::settings::SettingSpec;
use skwir_core::words;
use skwir_core::words::Words;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connector, Offered, Password, Recognised, Server};

pub const ID: &str = "remote";

/// The sidebar's section of remotes.
pub const GROUP: &str = skwir_core::registry::groups::NETWORK.id;

/// The sidebar's section of this machine's devices, where a phone or a
/// camera GVfs reads is.
const DEVICES: &str = skwir_core::registry::groups::DEVICES.id;

const CONNECTORS: &str = "remote.connectors";
const THUMBNAILS: &str = "remote.thumbnails";
const CHOSEN: &str = "remote.chosen";
/// The servers whose password Skwir keeps in the keyring, by their key
/// (`passwords::key`): never a password (ADR-082).
pub(crate) const REMEMBERED: &str = "remote.remembered";

/// The tools, in the order the user chose by default (2026-10-20: sshfs
/// first for SFTP, then GVfs, rclone, the system's mounts), KIO after GVfs
/// (ADR-081), KDE's kmtpd before GVfs for a phone it holds, which GVfs
/// cannot open (ADR-097)...
const ORDER: &str = "sshfs kmtp gvfs kio rclone fstab";

/// ...but KIO before GVfs on KDE's desktop, whose remotes and passwords
/// are KIO's.
const ORDER_ON_KDE: &str = "sshfs kio kmtp gvfs rclone fstab";

/// Off Linux, the system's own shares first, then rclone and sshfs (over
/// macFUSE).
const ORDER_OFF_LINUX: &str = "system rclone sshfs fstab gvfs kio kmtp";

/// The tools' order where the user chose none: the desktop's own first.
fn default_order() -> &'static str {
    order_on(&skwir_platform::apps::current_desktops())
}

/// [`default_order`] on the desktops `XDG_CURRENT_DESKTOP` lists.
fn order_on(desktops: &[String]) -> &'static str {
    if cfg!(any(windows, target_os = "macos")) {
        return ORDER_OFF_LINUX;
    }
    if desktops
        .iter()
        .any(|desktop| desktop.eq_ignore_ascii_case("KDE"))
    {
        ORDER_ON_KDE
    } else {
        ORDER
    }
}

/// A mount of the table, the connector that knows it, and what it is.
#[derive(Clone)]
struct Mounted {
    remote: Arc<Remote>,
    connector: Option<Arc<dyn Connector>>,
    recognised: Recognised,
    /// Whether the user may let go of it ([`can_disconnect`]), found as it
    /// was recognised, on the thread reading the table: what the sidebar
    /// offers is read from memory (`Remotes::publish`, under the state's
    /// lock, at times on the window's thread).
    ejects: bool,
}

#[derive(Default)]
struct State {
    /// Cancelled when the plugin is disabled.
    lifetime: Option<CancelToken>,
    /// The servers the tools' configurations name, as last read.
    servers: Vec<Server>,
    /// Whether each host is on the local network, as found out.
    local: HashMap<String, Option<bool>>,
    /// Mounts being let go of or sending what waits, by point, and what
    /// their places say meanwhile.
    busy: HashMap<String, Words>,
    /// The folders of servers a tool that mounts each path (kio-fuse)
    /// answered, by the server's address: where each is, which its place
    /// opens from then on (`as_mounted`). One below a mount that went (a
    /// kio-fuse started again mounts elsewhere) is asked again.
    answered: HashMap<String, PathBuf>,
    /// Points whose program is gone (`ENOTCONN`): drawn unavailable.
    lost: std::collections::HashSet<PathBuf>,
    /// The servers being connected to now, by their address's top, each
    /// with how many connect to it (`Connecting`): what a pane offering to
    /// connect again says in place of its button meanwhile (ADR-149, 4).
    connecting_to: HashMap<String, usize>,
    /// What reads proved of the mounts when last published
    /// (`fs::reach::proved_changed`): a plan's end that changed it
    /// publishes again.
    proved_seen: u64,
    /// Each mount's free and whole space, as its server says (measured
    /// in its lane).
    space: HashMap<PathBuf, (u64, u64)>,
    /// What the storage of each rclone mount Skwir made can do, as rclone
    /// says (ADR-086): asked once a mount, in its lane.
    features: HashMap<PathBuf, rclone_features::Features>,
    /// The files each rclone mount Skwir made still sends to its server,
    /// what other programs wrote there (ADR-102): said on its place while
    /// some wait (`Remotes::look_at_uploads`).
    waiting: HashMap<PathBuf, usize>,
    /// The mounts whose waiting files are being looked at again, every
    /// `UPLOADS_LOOKED_EVERY` until none waits.
    watching: std::collections::HashSet<PathBuf>,
    /// Bumped by each read of the mounts, and of the servers the tools
    /// know (`Remotes::read_again`), so a slower one never wins.
    mounts_generation: u64,
    servers_generation: u64,
    /// The read of the servers the tools know under way, cancelled by the
    /// next: a tool is waited for as long as it takes (a machine under
    /// load), never cut off by a time and its answer lost, and one that
    /// never answers is let go of when the servers are read again. `None`
    /// once it ended.
    servers_reading: Option<CancelToken>,
    /// Where the servers were read, followed while the plugin is enabled
    /// (`sources`).
    /// Behind a lock of their own: watching resolves each folder's path,
    /// never while this state's lock is held (`Sources::follow`).
    sources: Option<Arc<Mutex<sources::Sources>>>,
    /// The refreshes asked, whole and of the mounts alone
    /// (`Remotes::refreshes`): not what the system told.
    refreshes: (u64, u64),
    /// What each bridge held when last listed (`Bridges::Kept`), by its
    /// point, with when that listing began (`mounts_generation` then): a
    /// slower listing begun before never takes the place of a later one.
    bridged: HashMap<PathBuf, (u64, Vec<Arc<Remote>>)>,
    /// When the listing of each bridge `mounted` shows began: a read
    /// dropped as older that listed a bridge later reads the mounts again
    /// from its listing (`Remotes::read_again`).
    shown_bridges: HashMap<PathBuf, u64>,
    /// What a whole refresh owes, of the mounts and of the servers (all
    /// shown, every mount asked whether it answers), until the newest read
    /// ends: a read told by the system that began after it takes its
    /// place, and its due with it, its own read dropped as older.
    whole_owed: (bool, bool),
    /// The mounts as last read (`Remotes::mounted`): what every publish
    /// shows, whoever asks for it.
    mounted: Arc<Vec<Mounted>>,
    /// The remotes connected at login, by their entry's id (`login::id_of`).
    at_login: std::collections::HashSet<String>,
    /// The ids `at_login` was asked for: asked again only once what is
    /// listed names others, or a refresh is asked (on Windows each asking
    /// runs `schtasks`).
    login_ids: std::collections::HashSet<String>,
    /// The read as a window comes to the front under way
    /// (`Remotes::looked_at`): one at a time, never another started
    /// behind one a bridge's program holds.
    looking: bool,
    /// When the last of those began: one in `remotes::REFRESHED_WITHIN`,
    /// as the folders shown are read again.
    looked: Option<std::time::Instant>,
    /// Mounts come while no one looked, not asked yet whether they answer
    /// (`Remotes::read_again`): asked as a window comes to the front,
    /// whenever the last read then was.
    ask_owed: std::collections::HashSet<PathBuf>,
    /// Mounts whose program is being asked whether it still answers: one
    /// question at a time each, never another queued behind one a server
    /// gone holds for good.
    asking: std::collections::HashSet<PathBuf>,
    /// The devices plugged in each connector offers, mounted or not
    /// (ADR-096), as it last told.
    devices: HashMap<&'static str, Vec<Offered>>,
    /// Where each server's mounts were this run, by the address each
    /// mount is of, with what it was called: what a pane whose folder was
    /// there offers to connect again once it is not, a server kept or one
    /// typed in the address field alike (`Registry::reopens`, ADR-149). As
    /// few as the servers connected.
    mounted_at: Vec<(Address, PathBuf, Words)>,
    /// The mounts' points last published as known (`Registry::remotes`):
    /// one more or one fewer is told to the core (`App::volumes_changed`),
    /// the table not listing a bridge's folders.
    known_points: Vec<PathBuf>,
}

/// A server being connected to (`State::connecting_to`), said so where a
/// pane offers to connect to it again, until dropped, however the
/// connection ends.
struct Connecting<'a> {
    remotes: &'a Remotes,
    app: &'a App,
    top: String,
}

impl<'a> Connecting<'a> {
    fn begin(remotes: &'a Remotes, app: &'a App, address: &Address) -> Self {
        let top = address.top().to_string();
        *remotes
            .state
            .lock()
            .connecting_to
            .entry(top.clone())
            .or_default() += 1;
        remotes.publish(app);
        Connecting { remotes, app, top }
    }
}

impl Drop for Connecting<'_> {
    fn drop(&mut self) {
        {
            let mut state = self.remotes.state.lock();
            if let Some(count) = state.connecting_to.get_mut(&self.top) {
                *count -= 1;
                if *count == 0 {
                    state.connecting_to.remove(&self.top);
                }
            }
        }
        self.remotes.publish(self.app);
    }
}

/// What a pane whose folder was on the server `label` mounted at `at`
/// offers (`Registry::reopens`, ADR-149): to connect to `address` again,
/// the folder opened below it. `lost`: still mounted, its connection lost.
fn reopen(label: &Words, address: &Address, at: &Path, lost: bool) -> skwir_core::registry::Reopen {
    let name = || words::Named(label.clone());
    skwir_core::registry::Reopen {
        at: at.to_path_buf(),
        title: if lost {
            words!(
                remote_reopen_lost = "The connection to { $name } is lost",
                name = name(),
                note = "Shown in place of a folder's files when the connection to the server the folder is on was lost. name: the server, as the sidebar names it."
            )
        } else {
            words!(
                remote_reopen_not_connected = "{ $name } is not connected",
                name = name(),
                note = "Shown in place of a folder's files when the server the folder is on was disconnected. name: the server, as the sidebar names it."
            )
        },
        label: words!(
            remote_reopen_connect = "Connect to { $name }",
            name = name(),
            note = "The button in place of a folder's files whose server was disconnected: connect again and show the folder."
        ),
        command: Invocation::new("remote.connect").with("address", address.to_string()),
        busy: None,
    }
}

impl State {
    /// Keep what the bridge at `point` held, listed from `at`: never in
    /// place of a listing begun later.
    fn keep_listing(&mut self, point: PathBuf, at: u64, held: Vec<Arc<Remote>>) {
        match self.bridged.entry(point) {
            std::collections::hash_map::Entry::Occupied(kept) if kept.get().0 > at => {}
            entry => {
                entry.insert_entry((at, held));
            }
        }
    }

    /// Whether `used`, what a read showed of the bridges (each by when its
    /// listing began), holds a bridge still mounted listed later than
    /// `mounted` shows it.
    fn listed_later(&self, used: &HashMap<PathBuf, u64>) -> bool {
        used.iter().any(|(point, at)| {
            self.bridged.contains_key(point)
                && self.shown_bridges.get(point).is_none_or(|shown| shown < at)
        })
    }

    /// Whether each mount's server is near, where its name says so (an
    /// address, `nas.local`); a name to resolve is found out after. What
    /// names no server (a device, a bucket) is never looked up
    /// (`Address::server_name`).
    fn note_said(&mut self) {
        let mounted = Arc::clone(&self.mounted);
        for m in mounted.iter() {
            if let Some(host) = m.recognised.address.as_ref().and_then(Address::server_name)
                && !self.local.contains_key(host)
                && let Some(near) = lan::said(host)
            {
                self.local.insert(host.to_owned(), Some(near));
            }
        }
    }
}

/// How a read of the mounts takes what the bridges (GVfs's, kio-fuse's)
/// hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bridges {
    /// Each listed: GVfs said its mounts changed, the window came back, a
    /// refresh asked. Its program answers from memory, but waits while
    /// all it has waits on a server that stopped (Samba stopped under
    /// GVfs): what lists it waits too, on a thread of its own.
    Listed,
    /// As last listed, a bridge new to the table listed: a change of the
    /// table, which every process following it hears, never changes what
    /// a bridge holds, and reading it never waits on one.
    Kept,
}

/// Why the remotes are read again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// By the user, after an action of Skwir's, or a device said: all of
    /// it shown again, every mount asked whether it answers.
    Whole,
    /// Told that something changed outside Skwir (the table of mounts, a
    /// file the servers are read from) or the window come back: shown
    /// only where what it shows changed, only a mount come since asked
    /// whether it answers, and that only while someone looks (asked
    /// otherwise as the window comes back). A system busy mounting what is
    /// no remote (a container starting) costs a read in memory.
    Told,
}

/// What a read of the remotes holds until it ends, run or dropped unrun:
/// the read of the servers under way (`State::servers_reading`), no
/// longer unless a later one began, and the window's read
/// (`State::looking`).
struct ReadEnds {
    state: Arc<Mutex<State>>,
    servers_at: Option<u64>,
    looked: bool,
}

impl Drop for ReadEnds {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        if self.servers_at == Some(state.servers_generation) {
            state.servers_reading = None;
        }
        if self.looked {
            state.looking = false;
        }
    }
}

/// Whether `before` and `now` show the same mounts: each by its point,
/// type and source, the tool that knows it and what that tool said of it.
fn same_mounts(before: &[Mounted], now: &[Mounted]) -> bool {
    before.len() == now.len()
        && before.iter().zip(now).all(|(a, b)| {
            a.remote == b.remote
                && a.recognised == b.recognised
                && a.ejects == b.ejects
                && a.connector.as_ref().map(|c| c.id()) == b.connector.as_ref().map(|c| c.id())
        })
}

/// What the plugin keeps: its connectors and what it found.
#[derive(Clone)]
pub struct Remotes {
    connectors: Arc<Vec<Arc<dyn Connector>>>,
    state: Arc<Mutex<State>>,
    /// One connection at a time to each address's top: a second (two
    /// tabs restored on one server) waits for the first, then finds its
    /// mount, rather than asking the password twice and mounting twice.
    connecting: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Where servers' passwords are kept, when asked (ADR-082).
    keyring: Arc<dyn passwords::Keyring>,
    /// One publish at a time, each of the state as it then is: the last
    /// to run shows the newest mounts.
    publishing: Arc<Mutex<()>>,
    /// The tools whose devices are being listed again, by id: whether a
    /// change came meanwhile, which lists them once more after. Devices
    /// are followed on Linux alone, over the session bus.
    #[cfg(target_os = "linux")]
    relisting: Arc<Mutex<HashMap<&'static str, bool>>>,
    /// rclone's remotes as Settings lists them (ADR-086), told when they
    /// change.
    records: skwir_core::settings::RecordSource,
    /// rclone as its remotes are changed (ADR-086): its own configuration,
    /// the user's, or a test's.
    rclone: rclone::Rclone,
    /// ssh as key login is set up and sshfs runs at login: the user's own
    /// configuration, keys and agent, or a test's.
    keys: KeySetup,
    /// The service manager that connects remotes at login (ADR-087): the
    /// user's own, or a test's.
    login: Option<login::Manager>,
}

/// ssh as key login is set up (`keys`): its options, the folder of the
/// user's keys and the agent asked for its keys, each the user's own where
/// `None`.
#[derive(Clone, Debug, Default)]
pub struct KeySetup {
    pub ssh: keys::Ssh,
    pub ssh_dir: Option<PathBuf>,
    pub agent: Option<PathBuf>,
}

impl Default for Remotes {
    fn default() -> Self {
        Remotes::with(vec![
            Arc::new(sshfs::Sshfs::default()),
            Arc::new(kmtp::Kmtp::default()),
            Arc::new(gvfs::Gvfs::default()),
            Arc::new(kio::Kio::default()),
            Arc::new(rclone::Rclone::default()),
            Arc::new(fstab::Fstab),
            Arc::new(system_shares::SystemShares),
        ])
    }
}

impl Remotes {
    /// The cache Automatic chooses for `top`'s server, and why
    /// (`options::automatic`, ADR-102): by its kind, as rclone's
    /// configuration last said, and whether its server is on the local
    /// network (what names no server, a remote of rclone's configuration
    /// or an S3 bucket, is not asked: `Address::server_name`). May
    /// resolve a name for a moment: on a worker.
    pub fn automatic_cache(&self, top: &Address) -> (&'static str, &'static str) {
        let (kind, noted) = {
            let state = self.state.lock();
            let kind = state
                .servers
                .iter()
                .find(|server| server.address.top() == *top)
                .map(|server| server.kind.clone());
            let noted = top
                .server_name()
                .and_then(|host| state.local.get(host).copied().flatten());
            (kind, noted)
        };
        let near = top
            .server_name()
            .and_then(|host| noted.or_else(|| lan::local(host)));
        options::automatic(kind.as_deref(), near)
    }

    /// The refreshes asked so far, whole and of the mounts alone: what a
    /// test counts.
    #[doc(hidden)]
    pub fn refreshes(&self) -> (u64, u64) {
        self.state.lock().refreshes
    }

    /// The plugin with `connectors` (a test's).
    pub fn with(connectors: Vec<Arc<dyn Connector>>) -> Self {
        Remotes {
            connectors: Arc::new(connectors),
            state: Arc::default(),
            connecting: Arc::default(),
            keyring: Arc::new(passwords::Desktop),
            publishing: Arc::default(),
            #[cfg(target_os = "linux")]
            relisting: Arc::default(),
            records: skwir_core::settings::RecordSource::new(
                "remote.server_records",
                "remote.add_server",
                "remote.edit_server",
                "remote.remove_server",
            ),
            rclone: rclone::Rclone::default(),
            keys: KeySetup::default(),
            login: None,
        }
    }

    /// The plugin setting key login up as `keys` says (a test's ssh, keys
    /// and agent).
    #[must_use]
    pub fn with_keys(mut self, keys: KeySetup) -> Self {
        self.keys = keys;
        self
    }

    /// The plugin connecting at login through `manager` (a test's).
    #[must_use]
    pub fn with_login(mut self, manager: login::Manager) -> Self {
        self.login = Some(manager);
        self
    }

    /// The service manager connecting remotes at login.
    fn login_manager(&self) -> anyhow::Result<login::Manager> {
        match &self.login {
            Some(manager) => Ok(manager.clone()),
            None => login::Manager::user(),
        }
    }

    /// sshfs as it runs at login: with the ssh options key login is set up
    /// with.
    fn sshfs(&self) -> sshfs::Sshfs {
        sshfs::Sshfs {
            askpass: self.keys.ssh.askpass.clone(),
            ssh_options: self.keys.ssh.options.clone(),
            ssh_config: self.keys.ssh.config.clone(),
            ..sshfs::Sshfs::default()
        }
    }

    /// The plugin changing rclone's remotes as `rclone` says (a test's
    /// configuration): give its rclone connector the same.
    #[must_use]
    pub fn with_rclone(mut self, rclone: rclone::Rclone) -> Self {
        self.rclone = rclone;
        self
    }

    /// The plugin keeping servers' passwords in `keyring` (a test's).
    #[must_use]
    pub fn with_keyring(mut self, keyring: Arc<dyn passwords::Keyring>) -> Self {
        self.keyring = keyring;
        self
    }

    /// Connect `address` through `connector`, its server's password
    /// answered from the keyring first, what the user asked to remember
    /// kept once it served (ADR-082).
    fn connect_keyed(
        &self,
        app: &App,
        connector: &Arc<dyn Connector>,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<connector::Connected> {
        // rclone's configuration's password, where it is encrypted: the
        // keyring's first (ADR-086).
        let ask: &Arc<dyn Ask> = &(Arc::new(rclone_config::ConfigAsk::new(
            Arc::clone(ask),
            self.rclone.keyring(),
        )) as Arc<dyn Ask>);
        let keyed = Arc::new(passwords::Keyed::new(
            Arc::clone(ask),
            Arc::clone(&self.keyring),
            address.top(),
            cancel.clone(),
        ));
        let asked: Arc<dyn Ask> = keyed.clone();
        let top = address.top();
        let options = options::of(app, &top).mounting(self.automatic_cache(&top).0);
        let connected = connector
            .connect_with(address, &options, &asked, cancel)
            .map_err(|error| anyhow::Error::new(Words::of_error(error)))?;
        if let Some(noted) = keyed.served() {
            self.note_remembered(app, &noted);
        }
        Ok(connected)
    }

    fn lifetime(&self) -> Option<CancelToken> {
        self.state.lock().lifetime.clone()
    }

    /// The device offered at `address`, where one is (ADR-096).
    fn offered_at(&self, address: &Address) -> Option<Offered> {
        address.scheme.device()?;
        self.state
            .lock()
            .devices
            .values()
            .flatten()
            .find(|o| o.address.same_server(address))
            .cloned()
    }

    /// What the user calls `address`: a device by the name its program
    /// gives it ("Example Phone"), not by its serial; a server by its
    /// address.
    pub fn label_of(&self, address: &Address) -> Words {
        self.offered_at(address)
            .map_or_else(|| address.label(), |offered| offered.label())
    }

    /// Follow the devices the tools offer (ADR-096, ADR-097): each tool's
    /// listed at once, then again when one of its signals is said. One
    /// listener per bus, on a thread of its own: the session's on the
    /// program's one connection (`skwir_platform::bus`), a test's on its
    /// own; each listened to again on the next connection once the bus is
    /// started again (`bus::keep`), every tool listed again then; until
    /// `lifetime` ends.
    fn follow_devices(&self, app: &App, lifetime: &CancelToken) {
        let tools: Vec<Arc<dyn Connector>> = self
            .connectors
            .iter()
            .filter(|c| !c.device_signals().is_empty())
            .cloned()
            .collect();
        #[cfg(target_os = "linux")]
        {
            let mut buses: Vec<(Vec<(std::ffi::OsString, std::ffi::OsString)>, Vec<_>)> =
                Vec::new();
            for tool in tools {
                let env = tool.bus_env();
                match buses.iter_mut().find(|(on, _)| *on == env) {
                    Some((_, on_it)) => on_it.push(tool),
                    None => buses.push((env, vec![tool])),
                }
            }
            for (env, tools) in buses {
                let (this, weak, lifetime) = (self.clone(), app.weak(), lifetime.clone());
                let started = std::thread::Builder::new()
                    .name("skwir-devices".into())
                    .spawn(move || this.listen_for_devices(&weak, &env, &tools, &lifetime));
                if let Err(error) = started {
                    tracing::debug!("the devices the tools offer are not followed: {error}");
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (tools, app, lifetime);
        }
    }

    /// [`Remotes::follow_devices`] for the tools on the bus `env` names,
    /// on this thread: their signals listened to, then each tool listed,
    /// then each listed again when one of its own is said; all again on
    /// the next connection, the bus started again.
    #[cfg(target_os = "linux")]
    fn listen_for_devices(
        &self,
        app: &std::sync::Weak<App>,
        env: &[(std::ffi::OsString, std::ffi::OsString)],
        tools: &[Arc<dyn Connector>],
        lifetime: &CancelToken,
    ) {
        let connect = || {
            if env.is_empty() {
                skwir_platform::bus::session().map_err(anyhow::Error::from)
            } else {
                bus::connect(env, Duration::from_secs(10))
            }
        };
        let mut routes: Vec<(zbus::MatchRule<'static>, usize)> = Vec::new();
        for (at, tool) in tools.iter().enumerate() {
            for rule in tool.device_signals() {
                match zbus::MatchRule::try_from(rule.as_str()) {
                    Ok(rule) => routes.push((rule.into_owned(), at)),
                    Err(error) => tracing::debug!("{} says no rule: {error}", tool.id()),
                }
            }
        }
        let rules: Vec<_> = routes.iter().map(|(rule, _)| rule.clone()).collect();
        let list = |at: usize| {
            if let Some(app) = app.upgrade() {
                self.relist(&app, &tools[at]);
            }
        };
        skwir_platform::bus::keep(connect, lifetime, |bus| {
            skwir_platform::bus::listen(
                bus,
                rules.clone(),
                lifetime,
                || (0..tools.len()).for_each(list),
                |message| {
                    let mut said: Vec<usize> = routes
                        .iter()
                        .filter(|(rule, _)| rule.matches(message).unwrap_or(false))
                        .map(|(_, at)| *at)
                        .collect();
                    said.dedup();
                    said.into_iter().for_each(list);
                },
            );
        });
    }

    /// Ask `tool` again what devices it offers, on a thread of its own (it
    /// may wait on a program): one asking at a time per tool, a change said
    /// meanwhile asked once more after.
    #[cfg(target_os = "linux")]
    fn relist(&self, app: &App, tool: &Arc<dyn Connector>) {
        {
            let mut relisting = self.relisting.lock();
            if let Some(again) = relisting.get_mut(tool.id()) {
                *again = true;
                return;
            }
            relisting.insert(tool.id(), false);
        }
        let id = tool.id();
        let (this, weak, tool) = (self.clone(), app.weak(), Arc::clone(tool));
        let started = std::thread::Builder::new()
            .name("skwir-devices-list".into())
            .spawn(move || {
                loop {
                    let devices = tool.devices();
                    if let Some(app) = weak.upgrade() {
                        this.offered(&app, tool.id(), devices);
                    }
                    let mut relisting = this.relisting.lock();
                    if relisting.get(tool.id()).copied().unwrap_or(false) {
                        relisting.insert(tool.id(), false);
                        continue;
                    }
                    relisting.remove(tool.id());
                    break;
                }
            });
        if started.is_err() {
            self.relisting.lock().remove(id);
        }
    }

    /// What `connector` offers now (ADR-096): the mounts are read again,
    /// a device gone, mounted or let go of with it.
    #[cfg(target_os = "linux")]
    fn offered(&self, app: &App, connector: &'static str, devices: Vec<Offered>) {
        {
            let mut state = self.state.lock();
            if state.lifetime.is_none() {
                return;
            }
            state.devices.insert(connector, devices);
        }
        reach::changed();
        self.refresh_mounts(app);
    }

    /// The connectors in the order the setting says, those it leaves out
    /// after, in their own.
    fn ordered(&self, app: &App) -> Vec<Arc<dyn Connector>> {
        let order: String = app
            .settings
            .get(CONNECTORS)
            .unwrap_or_else(|| default_order().to_owned());
        let mut ordered: Vec<Arc<dyn Connector>> = order
            .split([' ', ','])
            .filter(|id| !id.is_empty())
            .filter_map(|id| self.connectors.iter().find(|c| c.id() == id).cloned())
            .collect();
        for connector in self.connectors.iter() {
            if !ordered.iter().any(|c| c.id() == connector.id()) {
                ordered.push(Arc::clone(connector));
            }
        }
        ordered
    }

    /// Every network mount there is, each by the connector that knows it:
    /// a bridge's (GVfs's, kio-fuse's) one by one. Lists the bridges, which
    /// answer from memory: on a worker.
    fn mounted(&self) -> Vec<Mounted> {
        self.mounted_with(Bridges::Listed).0
    }

    /// [`Remotes::mounted`], what the bridges hold listed or as last
    /// listed (`bridges`), and when the listing of each bridge it shows
    /// began (`State::shown_bridges`).
    fn mounted_with(&self, bridges: Bridges) -> (Vec<Mounted>, HashMap<PathBuf, u64>) {
        let (offered, kept, at) = {
            let state = self.state.lock();
            (
                state
                    .devices
                    .values()
                    .flatten()
                    .cloned()
                    .collect::<Vec<Offered>>(),
                state.bridged.clone(),
                state.mounts_generation,
            )
        };
        let table = reach::all();
        let mut listed: HashMap<PathBuf, Vec<Arc<Remote>>> = HashMap::new();
        let mut used: HashMap<PathBuf, u64> = HashMap::new();
        let held: Vec<Arc<Remote>> = table
            .iter()
            .flat_map(|remote| {
                if !reach::is_bridge(remote) {
                    return vec![Arc::clone(remote)];
                }
                if let (Bridges::Kept, Some((listed_at, held))) = (bridges, kept.get(&remote.point))
                {
                    used.insert(remote.point.clone(), *listed_at);
                    return held.clone();
                }
                let held = reach::bridged(remote);
                listed.insert(remote.point.clone(), held.clone());
                used.insert(remote.point.clone(), at);
                held
            })
            .collect();
        {
            let mut state = self.state.lock();
            for (point, held) in listed {
                state.keep_listing(point, at, held);
            }
            let bridges: std::collections::HashSet<&Path> = table
                .iter()
                .filter(|r| reach::is_bridge(r))
                .map(|r| r.point.as_path())
                .collect();
            state
                .bridged
                .retain(|point, _| bridges.contains(point.as_path()));
        }
        let mounted = held
            .into_iter()
            .filter_map(|remote| {
                let known = self.connectors.iter().find_map(|c| {
                    let mut recognised = c.recognises(&remote)?;
                    // A device by the name its volume has, mounted or not.
                    if let Some(offered) = recognised.address.as_ref().and_then(|a| {
                        offered
                            .iter()
                            .find(|o| a.scheme.device().is_some() && o.address.same_server(a))
                    }) {
                        recognised.label = offered.label();
                    }
                    Some((Arc::clone(c), recognised))
                });
                Some(match known {
                    Some((connector, recognised)) => Mounted {
                        ejects: can_disconnect(connector.as_ref(), &remote, &recognised),
                        remote,
                        connector: Some(connector),
                        recognised,
                    },
                    // What kio-fuse holds of no remote (an archive KIO
                    // reads, a local folder) is no mount to show.
                    None if remote.fstype == kio::BRIDGE => return None,
                    None => Mounted {
                        recognised: Recognised {
                            label: Words::as_written(remote.point.file_name().map_or_else(
                                || remote.source.to_string_lossy().into_owned(),
                                |n| n.to_string_lossy().into_owned(),
                            )),
                            address: None,
                            ours: false,
                            device: None,
                        },
                        remote,
                        connector: None,
                        ejects: false,
                    },
                })
            })
            .collect();
        (mounted, used)
    }

    /// Where `address` is on this machine: in a mount there already, else
    /// in one the first connector able to opens. Blocks (it may ask the
    /// user): on a thread of its own.
    ///
    /// # Errors
    /// Why it could not be reached, in the user's words: for a device
    /// another program holds, which program (ADR-096).
    pub fn reach(
        &self,
        app: &App,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<PathBuf> {
        self.reach_or_mount(app, address, ask, cancel)
            .map_err(|error| match self.held_by(address) {
                Some(held) if !cancel.is_cancelled() => anyhow::anyhow!(held),
                _ => error,
            })
    }

    /// Why the device at `address` opens nowhere, where another program of
    /// the user's holds it open (KDE's `kiod6`, a camera's program): one
    /// program at a time uses a device over USB. Reads `/proc`: on a
    /// worker, once opening it failed.
    fn held_by(&self, address: &Address) -> Option<Words> {
        let device = address.scheme.device()?;
        let node = self.offered_at(address)?.node?;
        // GVfs's own program for it, still letting go of the open that
        // failed, is not another's (one for a camera holding a phone is).
        let asked = gvfs::backend_of(address.scheme);
        let mut programs = holders::of(&node);
        programs.retain(|program| *program != asked);
        if programs.is_empty() {
            return None;
        }
        let count = programs.len();
        // Programs by their names, as they were started.
        let them = listed(programs.iter().cloned().map(Words::as_written).collect());
        // What a language chooses its words by.
        let device = match device {
            connector::Device::Phone => "phone",
            connector::Device::Camera => "camera",
        };
        // A tool that would open it through the program holding it (KDE's
        // kmtpd, through `kiod6`), and cannot here: why, before quitting.
        let through = self.connectors.iter().find_map(|c| {
            let holder = c.reaches_through()?;
            programs
                .iter()
                .any(|p| p == holder)
                .then(|| c.unavailable().map(|why| (holder, why)))
                .flatten()
        });
        Some(match through {
            Some((holder, why)) => words!(
                remote_device_held_through = "{ $count ->
                    [1] { $programs } has
                   *[other] { $programs } have
                } it open, and only one program at a time can use { $device ->
                    [phone] a phone
                   *[camera] a camera
                }. Skwir would open it through { $holder }, but { $why }; else quit { $count ->
                    [1] { $programs }
                   *[other] them
                }, then try again.",
                count = count,
                programs = them,
                device = device,
                holder = holder,
                why = why,
                note = "Why a device plugged in opens nowhere. programs: the programs that hold \
                        it, by their names; device: phone or camera; holder: the program a tool \
                        would open it through (kiod6); why: why that tool cannot here."
            ),
            None => words!(
                remote_device_held = "{ $count ->
                    [1] { $programs } has
                   *[other] { $programs } have
                } it open, and only one program at a time can use { $device ->
                    [phone] a phone
                   *[camera] a camera
                }. Quit { $count ->
                    [1] { $programs }
                   *[other] them
                }, then try again.",
                count = count,
                programs = them,
                device = device,
                note = "Why a device plugged in opens nowhere. programs: the programs that hold \
                        it, by their names; device: phone or camera."
            ),
        })
    }

    /// [`Remotes::reach`], what failed as each tool said it.
    fn reach_or_mount(
        &self,
        app: &App,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<PathBuf> {
        let turn = Arc::clone(
            self.connecting
                .lock()
                .entry(address.top().to_string())
                .or_default(),
        );
        let _turn = loop {
            if let Some(held) = turn.try_lock_for(Duration::from_millis(100)) {
                break held;
            }
            anyhow::ensure!(!cancel.is_cancelled(), connector::cancelled());
        };
        for mounted in self.mounted() {
            if let Some(below) = mounted
                .recognised
                .address
                .as_ref()
                .and_then(|top| top.below(address))
            {
                // A mount whose program died, or whose connection was cut
                // (sshfs keeps its mount), answers nothing: mounted again.
                // A kio-fuse gone is let go of whole: the bus starts another,
                // which mounts in a folder of its own.
                if cut_off(&mounted.remote) {
                    let point = if mounted.remote.fstype == kio::BRIDGE {
                        reach::all()
                            .iter()
                            .find(|r| mounted.remote.point.starts_with(&r.point))
                            .map_or_else(|| mounted.remote.point.clone(), |r| r.point.clone())
                    } else {
                        mounted.remote.point.clone()
                    };
                    let _ = mounter::force_unmount(&point);
                    break;
                }
                // Said lost, it answers again (sshfs reconnected as its
                // server came back): no longer offered, and how full it is
                // asked again.
                if self.state.lock().lost.remove(&mounted.remote.point) {
                    self.publish(app);
                    let point = mounted.remote.point.clone();
                    self.ask_again(app, |at| at == point);
                }
                match &mounted.connector {
                    // A server kio-fuse holds, but maybe not this folder of
                    // it yet: asked for it.
                    Some(connector) if connector.mounts_each_path() => {
                        // Asked for a folder it does not hold yet, it
                        // connects: said so meanwhile, as below.
                        let connecting = Connecting::begin(self, app, address);
                        let connected = self.connect_keyed(app, connector, address, ask, cancel);
                        drop(connecting);
                        let connected = connected?;
                        let below = connected.address.below(address).unwrap_or_default();
                        let path = join(&connected.point, below);
                        // A server's folder answered: its place opens it
                        // there from now on, named by it in the trail and
                        // the tab.
                        let mut state = self.state.lock();
                        if state.servers.iter().any(|s| s.address == *address)
                            && state.answered.get(&address.to_string()) != Some(&path)
                        {
                            state.answered.insert(address.to_string(), path.clone());
                            drop(state);
                            self.publish(app);
                        }
                        return Ok(path);
                    }
                    _ => return Ok(join(&mounted.remote.point, below)),
                }
            }
        }
        // Said where a pane offers to connect to it again, until it ends.
        let _connecting = Connecting::begin(self, app, address);
        let mut why_not = Vec::new();
        let mut failed: Vec<(Words, anyhow::Error)> = Vec::new();
        for connector in self.ordered(app) {
            if !connector.schemes().contains(&address.scheme) {
                continue;
            }
            if let Some(reason) = connector.unavailable() {
                why_not.push(reason);
                continue;
            }
            if !connector.opens(address) {
                continue;
            }
            let heard = Arc::new(Heard {
                ask: Arc::clone(ask),
                asked: std::sync::atomic::AtomicBool::new(false),
            });
            let asked: Arc<dyn Ask> = heard.clone();
            let connected = match self.connect_keyed(app, &connector, address, &asked, cancel) {
                Ok(connected) => connected,
                // Stopped, or refused once the user was asked: that answer
                // stands, never asked again by the next tool.
                Err(error) if cancel.is_cancelled() || heard.asked() => return Err(error),
                // A tool that could not reach it without asking (GVfs
                // finding no NFS export where fstab names one) leaves it
                // to the next one installed.
                Err(error) => {
                    failed.push((connector.title(), error));
                    continue;
                }
            };
            reach::changed();
            self.publish_mounted(app);
            let below = connected.address.below(address).unwrap_or_default();
            let path = join(&connected.point, below);
            self.refresh(app);
            return Ok(path);
        }
        if failed.len() == 1 {
            return Err(failed.remove(0).1);
        }
        // Failures are said together, each by its tool, as a list.
        if !failed.is_empty() {
            let said: Vec<Words> = failed
                .into_iter()
                .map(|(tool, error)| {
                    words!(
                        remote_tool_failed = "{ $tool }: { $error }",
                        tool = tool,
                        error = error,
                        note = "What one tool could not do, among the failures of the tools \
                                tried. tool: its name (sshfs, GVfs); error: why."
                    )
                })
                .collect();
            anyhow::bail!(words!(
                remote_tools_failed = "{ $failures }",
                failures = listed(said),
                note = "Why a server was not reached, after Could not connect to …: what each \
                        tool tried could not do (remote_tool_failed), a list (sshfs: the server \
                        refused the login and rclone: the server's name was not found)."
            ));
        }
        if why_not.is_empty() {
            anyhow::bail!(words!(
                remote_no_tool = "no tool Skwir knows opens { $protocol } addresses",
                protocol = address.scheme.title(),
                note = "protocol: SFTP, SMB, MTP…"
            ));
        }
        anyhow::bail!(words!(
            remote_nothing_installed = "nothing installed can open it: { $why }",
            why = listed(why_not),
            note = "why: why each tool that opens such addresses cannot here, a list (sshfs \
                    is not installed and rclone is not installed)."
        ))
    }

    /// Read again what is mounted and publish it, then what servers are
    /// known: on a worker. What is mounted is read from memory and
    /// published at once; the servers the tools know (`rclone
    /// listremotes`, ssh's configuration) and whether each mount's server
    /// is near take longer, and are published after, unless a later
    /// refresh began meanwhile: the mounts never wait on them.
    fn refresh(&self, app: &App) {
        self.read_again(app, true, Asked::Whole, Bridges::Listed, false);
    }

    /// Read again what is mounted, and publish it, without the servers the
    /// tools know: a device said, or a mount GVfs made or let go of, which
    /// changes no tool's configuration (and `rclone listremotes` is a
    /// program started).
    #[cfg(target_os = "linux")]
    fn refresh_mounts(&self, app: &App) {
        self.read_again(app, false, Asked::Whole, Bridges::Listed, false);
    }

    /// The system's table of mounts changed (`reach::follow`), whoever
    /// mounted or let go: what is mounted is read again, and shown where
    /// it changed.
    fn mounts_told(&self, app: &App) {
        self.read_again(app, false, Asked::Told, Bridges::Kept, false);
    }

    /// A file the servers are read from changed (`sources`), whoever
    /// wrote it: the servers are read again, and shown where they changed.
    fn sources_told(&self, app: &App) {
        self.read_again(app, true, Asked::Told, Bridges::Kept, false);
    }

    /// The window came to the front: what no watch tells is read again
    /// (a folder kio-fuse holds more of, a share another program
    /// connected), shown where it changed; the servers too where a source
    /// could not be watched (`Sources::misses`), unless a read of them is
    /// under way. One at a time, and once in `remotes::REFRESHED_WITHIN`:
    /// a window gone back and forth, or several windows in turn, list the
    /// bridges once, and one a program holds holds no more than one read.
    fn looked_at(&self, app: &App) {
        let owed = std::mem::take(&mut self.state.lock().ask_owed);
        if !owed.is_empty() {
            self.ask_again(app, |point| owed.contains(point));
        }
        let servers = {
            let mut state = self.state.lock();
            if state.looking
                || state
                    .looked
                    .is_some_and(|at| at.elapsed() < skwir_core::remotes::REFRESHED_WITHIN)
            {
                return;
            }
            state.looking = true;
            state.looked = Some(std::time::Instant::now());
            state.servers_reading.is_none()
                && state
                    .sources
                    .as_ref()
                    .is_none_or(|sources| sources.lock().misses())
        };
        self.read_again(app, servers, Asked::Told, Bridges::Listed, true);
    }

    /// [`Remotes::refresh`], the servers read again where `servers`, else
    /// [`Remotes::refresh_mounts`]. Each kind of read stores only while no
    /// later one of its kind began: a read of the mounts alone never drops
    /// the servers a whole refresh reads.
    fn read_again(&self, app: &App, servers: bool, asked: Asked, bridges: Bridges, looked: bool) {
        // Held until the read ends, run or dropped unrun.
        let mut ends = ReadEnds {
            state: Arc::clone(&self.state),
            servers_at: None,
            looked,
        };
        let Some(lifetime) = self.lifetime() else {
            return;
        };
        let (mounts_at, servers_at, reading) = {
            let mut state = self.state.lock();
            state.mounts_generation += 1;
            // Owed by whichever read is the newest once it ends: a later
            // one, told, takes its place.
            if asked == Asked::Whole {
                state.whole_owed.0 = true;
                state.whole_owed.1 |= servers;
            }
            let reading = if servers {
                state.servers_generation += 1;
                if asked == Asked::Whole {
                    state.refreshes.0 += 1;
                }
                let reading = lifetime.child();
                if let Some(before) = state.servers_reading.replace(reading.clone()) {
                    before.cancel();
                }
                reading
            } else {
                if asked == Asked::Whole {
                    state.refreshes.1 += 1;
                }
                lifetime.clone()
            };
            (state.mounts_generation, state.servers_generation, reading)
        };
        // The read of the servers under way no longer once this one ends,
        // unless a later one began.
        ends.servers_at = servers.then_some(servers_at);
        let this = self.clone();
        let weak = app.weak();
        let token = lifetime.clone();
        let refresh = move || {
            let _ends = ends;
            if lifetime.is_cancelled() {
                return;
            }
            let (mounted, used) = this.mounted_with(bridges);
            let mounted = Arc::new(mounted);
            let (newest, before, whole, fresher) = {
                let mut state = this.state.lock();
                let newest = state.mounts_generation == mounts_at;
                let before = Arc::clone(&state.mounted);
                // Dropped as older, but a bridge listed later than what the
                // newest read showed (that read kept it as listed before,
                // `Bridges::Kept`): the mounts are read again from this
                // listing, never lost to it.
                let fresher = !newest && state.listed_later(&used);
                // What a whole refresh this one took the place of owed.
                let whole =
                    newest && (std::mem::take(&mut state.whole_owed.0) || asked == Asked::Whole);
                if newest {
                    state.mounted = Arc::clone(&mounted);
                    state.shown_bridges = used;
                    state.note_said();
                    state
                        .lost
                        .retain(|point| mounted.iter().any(|m| m.remote.point == *point));
                    state
                        .space
                        .retain(|point, _| mounted.iter().any(|m| m.remote.point == *point));
                    state
                        .features
                        .retain(|point, _| mounted.iter().any(|m| m.remote.point == *point));
                }
                (newest, before, whole, fresher)
            };
            if fresher && let Some(app) = weak.upgrade() {
                this.mounts_told(&app);
            }
            // A read-only volume the way to where the servers are read
            // goes through, mounted again in its place (NixOS's `/etc` as
            // an overlay, at a switch): its links name other files, and no
            // watch there told it (`sources::remounted`).
            if !servers && asked == Asked::Told {
                let sources = this.state.lock().sources.clone();
                let fixed = sources
                    .map(|sources| sources.lock().fixed())
                    .unwrap_or_default();
                if sources::remounted(&fixed)
                    && let Some(app) = weak.upgrade()
                {
                    this.sources_told(&app);
                }
            }
            // A later whole refresh reads the servers: not this one.
            let servers = servers && this.state.lock().servers_generation == servers_at;
            if !newest && !servers {
                return;
            }
            let Some(app) = weak.upgrade() else {
                return;
            };
            // Told: shown where it changed. A mount come since is asked
            // whether it answers only while someone looks: asking costs its
            // server a request, and holds the mount a moment, just as the
            // program that made it may let go of it again (a connection
            // refused, tried again); one not asked so is asked as a window
            // next comes to the front (`State::ask_owed`), and each mount
            // once in a while then (`Known::looked_at`). A refresh asked
            // asks every mount; one
            // a told read took the place of, those it knew.
            let changed = !same_mounts(&before, &mounted);
            if newest && (whole || changed) {
                this.publish(&app);
                if asked == Asked::Whole {
                    this.ask_alive(&app, mounted.iter(), &lifetime);
                } else {
                    let known: std::collections::HashSet<&Path> =
                        before.iter().map(|m| m.remote.point.as_path()).collect();
                    let looked_at = app.looked_at();
                    let mut owed = Vec::new();
                    this.ask_alive(
                        &app,
                        mounted.iter().filter(|m| {
                            if known.contains(m.remote.point.as_path()) {
                                whole
                            } else {
                                if !looked_at {
                                    owed.push(m.remote.point.clone());
                                }
                                looked_at
                            }
                        }),
                        &lifetime,
                    );
                    // Asked as a window comes to the front, whenever.
                    this.state.lock().ask_owed.extend(owed);
                }
            }
            // The servers kept (Skwir's list, KDE's network folders),
            // then the tools' that are not those: read again where
            // asked, else as last read; and where they were read, which is
            // followed from then on.
            let listed = servers.then(|| {
                let mut listed = servers::listed(&app);
                let mut from = servers::sources();
                for connector in this.connectors.iter() {
                    if connector.unavailable().is_none() {
                        let named = connector.servers(&reading);
                        from.extend(named.from);
                        for server in named.servers {
                            if !listed.iter().any(|s| s.address == server.address) {
                                listed.push(server);
                            }
                        }
                    }
                }
                (listed, from)
            });
            drop(app);
            let (listed, from) = listed.unzip();
            // The way to where they were read, walked here: what the lock
            // below holds is only the watching.
            let from = from.map(sources::Followed::of);
            // Whether each server is near, for its thumbnails. What
            // names no server is never looked up (`Address::server_name`:
            // a device, near, its name a serial; a remote of rclone's
            // configuration, an S3 bucket, names not hosts): looked up,
            // each went to the network's resolvers and took the whole wait
            // (`lan::RESOLVING`), one after another, before the servers
            // were published. What is known of a host stays known.
            let known = this.state.lock().local.clone();
            let near: Vec<(String, Option<bool>)> = mounted
                .iter()
                .filter_map(|m| m.recognised.address.as_ref()?.server_name())
                .filter(|host| !known.contains_key(*host))
                .map(|host| (host.to_owned(), lan::local(host)))
                .collect();
            // Which of them connect at login: their manager's files (on
            // Windows its program, run), asked again once what is listed
            // names others, or a refresh was asked (Skwir's own changes
            // to them ask one): never for what changed nothing here.
            let (asked_ids, kept) = {
                let state = this.state.lock();
                (
                    state.login_ids.clone(),
                    listed.clone().unwrap_or_else(|| state.servers.clone()),
                )
            };
            let ids: std::collections::HashSet<String> = mounted
                .iter()
                .filter_map(|m| m.recognised.address.as_ref())
                .chain(kept.iter().map(|s| &s.address))
                .filter_map(configure::login_id)
                .collect();
            let at_login = (whole || asked == Asked::Whole || ids != asked_ids).then(|| {
                let at_login = this.login_manager().map_or_else(
                    |_| std::collections::HashSet::new(),
                    |manager| manager.installed_of(ids.iter().cloned()),
                );
                (at_login, ids)
            });
            let found = !near.is_empty();
            let (shown, follow) = {
                let mut state = this.state.lock();
                for (host, near) in near {
                    state.local.entry(host).or_insert(near);
                }
                let newest = if servers {
                    state.servers_generation == servers_at
                } else {
                    state.mounts_generation == mounts_at
                };
                if !newest {
                    return;
                }
                // Followed once this lock is let go of (`Sources::follow`).
                let follow = from.zip(state.sources.clone());
                let whole = whole
                    || (servers
                        && (std::mem::take(&mut state.whole_owed.1) || asked == Asked::Whole));
                let listed = listed.unwrap_or_else(|| state.servers.clone());
                let (at_login, ids) =
                    at_login.unwrap_or_else(|| (state.at_login.clone(), state.login_ids.clone()));
                let kept_changed = state.servers != listed || state.at_login != at_login;
                if kept_changed {
                    this.records.changed();
                }
                state.servers = listed;
                state.at_login = at_login;
                state.login_ids = ids;
                (whole || kept_changed || found, follow)
            };
            // A folder watched only now may have changed since this read
            // (a file replaced as its folder was made): read once more, the
            // watch in place, rather than wait for a change that came.
            let watched_late =
                follow.is_some_and(|(from, sources)| sources.lock().follow(from, servers_at));
            if shown && let Some(app) = weak.upgrade() {
                this.publish(&app);
            }
            if watched_late && let Some(app) = weak.upgrade() {
                this.sources_told(&app);
            }
        };
        if servers || bridges == Bridges::Listed {
            // A tool asked its servers is waited for as long as it takes,
            // until the next read of them cancels it (`Connector::servers`),
            // and a bridge listed may wait on its program (all it has waits
            // on a server that stopped): on a thread of its own, never
            // holding one of the workers every folder shares meanwhile.
            if let Err(error) = std::thread::Builder::new()
                .name("skwir-remote-servers".into())
                .spawn(refresh)
            {
                tracing::warn!("no thread to read the servers ({error})");
            }
        } else {
            app.scheduler
                .spawn(Priority::Background, "remote-refresh", token, refresh);
        }
    }

    /// Ask each of `mounted` whether its program still answers, and how
    /// full it is, in the remote's own lane: a server that stopped
    /// answering holds that alone (ADR-077). A mount still being asked is
    /// not asked again.
    fn ask_alive<'a>(
        &self,
        app: &App,
        mounted: impl IntoIterator<Item = &'a Mounted>,
        lifetime: &CancelToken,
    ) {
        /// The mount being asked, no longer once its question ran, or was
        /// dropped unrun.
        struct Asking(Arc<Mutex<State>>, PathBuf);
        impl Drop for Asking {
            fn drop(&mut self) {
                self.0.lock().asking.remove(&self.1);
            }
        }
        let weak = app.weak();
        for m in mounted {
            let point = m.remote.point.clone();
            {
                let mut state = self.state.lock();
                state.ask_owed.remove(&point);
                if !state.asking.insert(point.clone()) {
                    continue;
                }
            }
            let asking = Asking(Arc::clone(&self.state), point.clone());
            let (this, weak) = (self.clone(), weak.clone());
            // What an rclone mount of Skwir's can do, asked once a mount.
            let ask_features = m.recognised.ours
                && m.connector.as_ref().is_some_and(|c| c.id() == "rclone")
                && !self.state.lock().features.contains_key(&point);
            let lifetime_of_ask = lifetime.clone();
            let remote = m.remote.clone();
            app.scheduler.spawn_in(
                m.remote.lane(),
                Priority::Background,
                "remote-alive",
                lifetime.clone(),
                move || {
                    let _asking = asking;
                    let gone = cut_off(&remote);
                    // Answering: what copies cut short left there goes.
                    if !gone && let Some(app) = weak.upgrade() {
                        app.sweep_left_on(&point);
                    }
                    let features = (ask_features && !gone)
                        .then(|| rclone_features::of_mount(&point, &lifetime_of_ask))
                        .flatten();
                    if let Some(features) = &features {
                        this.state
                            .lock()
                            .features
                            .insert(point.clone(), features.clone());
                    }
                    // How full it is, as the server says: asked here too,
                    // where waiting holds no one else. Not where rclone
                    // says its storage cannot tell: its mount makes a size
                    // up.
                    let tells = this
                        .state
                        .lock()
                        .features
                        .get(&point)
                        .is_none_or(|f| f.about);
                    let space = if gone || !tells {
                        None
                    } else {
                        skwir_platform::places::space(&point)
                    };
                    let changed = {
                        let mut state = this.state.lock();
                        let lost = if gone {
                            state.lost.insert(point.clone())
                        } else {
                            state.lost.remove(&point)
                        };
                        let measured = match space {
                            Some(space) => state.space.insert(point, space) != Some(space),
                            None => state.space.remove(&point).is_some(),
                        };
                        lost || measured
                    };
                    if changed && let Some(app) = weak.upgrade() {
                        this.publish(&app);
                    }
                },
            );
        }
    }

    /// Ask the mounts whose point `which` names again whether they answer
    /// and how full they are (`ask_alive`): as the window comes back
    /// (`Known::looked_at`), and as a plan that wrote there or freed room
    /// there ends. No rescan follows what fills a server.
    fn ask_again(&self, app: &App, which: impl Fn(&Path) -> bool) {
        let (mounted, lifetime) = {
            let state = self.state.lock();
            (Arc::clone(&state.mounted), state.lifetime.clone())
        };
        if let Some(lifetime) = lifetime {
            self.ask_alive(
                app,
                mounted.iter().filter(|m| which(&m.remote.point)),
                &lifetime,
            );
        }
    }

    /// Read what is mounted and publish it at once, on this thread: a
    /// connection was just made, its folder is shown next, and F5 there
    /// must find what its program is told (`Known::forget`). A refresh
    /// that began before stores nothing older.
    fn publish_mounted(&self, app: &App) {
        let (mounted, used) = self.mounted_with(Bridges::Listed);
        {
            let mut state = self.state.lock();
            state.mounts_generation += 1;
            state.mounted = Arc::new(mounted);
            state.shown_bridges = used;
            state.note_said();
        }
        self.publish(app);
    }

    /// Put the mounts and the servers in the sidebar, and what is known of
    /// each mount where the core asks (`Registry::remotes`): the state's
    /// as it is now. Nothing once the plugin is disabled (a task still
    /// running then).
    fn publish(&self, app: &App) {
        let _publishing = self.publishing.lock();
        let mut state = self.state.lock();
        if state.lifetime.is_none() {
            return;
        }
        let mounted = Arc::clone(&state.mounted);
        // Where each server is mounted now, kept for when it is not.
        for m in mounted.iter() {
            if let Some(address) = &m.recognised.address {
                state.mounted_at.retain(|(a, ..)| a != address);
                state.mounted_at.push((
                    address.clone(),
                    m.remote.point.clone(),
                    m.recognised.label.clone(),
                ));
            }
        }
        // Each mounted earlier this run that is not now offers to connect
        // again; a mount whose connection is lost, too. What is done to it
        // meanwhile (Connecting…, Disconnecting…) is said in place of its
        // button, as its place says it (shell.md).
        let doing = |address: &Address, at: &Path| {
            state.busy.get(&at.display().to_string()).cloned().or_else(|| {
                state
                    .connecting_to
                    .contains_key(&address.top().to_string())
                    .then(|| {
                        words!(
                            remote_connecting = "Connecting…",
                            note = "Said in place of the Connect button shown where a folder's server is not connected, while Skwir connects to it."
                        )
                    })
            })
        };
        let offer = |label: &Words, address: &Address, at: &Path, lost: bool| {
            skwir_core::registry::Reopen {
                busy: doing(address, at),
                ..reopen(label, address, at, lost)
            }
        };
        let mut reopens: Vec<skwir_core::registry::Reopen> = state
            .mounted_at
            .iter()
            .filter(|(address, ..)| {
                !mounted
                    .iter()
                    .any(|m| m.recognised.address.as_ref() == Some(address))
            })
            .map(|(address, at, label)| offer(label, address, at, false))
            .collect();
        reopens.extend(mounted.iter().filter_map(|m| {
            let address = m.recognised.address.as_ref()?;
            state
                .lost
                .contains(&m.remote.point)
                .then(|| offer(&m.recognised.label, address, &m.remote.point, true))
        }));
        // Read before what it words: a change after it publishes again.
        let proved = skwir_core::fs::reach::proved_changed();
        let chosen: HashMap<String, String> = app.settings.get(CHOSEN).unwrap_or_default();
        let remembered: Vec<String> = app.settings.get(REMEMBERED).unwrap_or_default();
        let unfinished = rclone_forms::unfinished(app);
        let default: String = app
            .settings
            .get(THUMBNAILS)
            .unwrap_or_else(|| "automatic".to_owned());
        let mut places = Vec::new();
        let mut known = Vec::new();
        // A server kept with a folder below a mount's top is a place of
        // its own, opening that folder (`as_mounted`); a mount only such
        // servers hold gives way to them. A device's mount stays the
        // device's place, a server beside it, and a server at a mount's
        // top is drawn as the mount's own place, as is a mount no server
        // names.
        let held = held(&state.servers, &mounted);
        let Holds { in_folder, at_top } = holds(&held, &mounted);
        let gives_way = |at: usize| {
            in_folder[at] && at_top[at].is_none() && mounted[at].recognised.device.is_none()
        };
        let mut of_servers: Vec<Option<Place>> = vec![None; mounted.len()];
        for (at, m) in mounted.iter().enumerate() {
            let point = &m.remote.point;
            let address = m.recognised.address.as_ref();
            let key = address.map_or_else(|| point.display().to_string(), ToString::to_string);
            let rule = chosen.get(&key).unwrap_or(&default);
            let near = if m.recognised.device.is_some() {
                Some(true)
            } else {
                address
                    .and_then(Address::server_name)
                    .and_then(|host| state.local.get(host).copied().flatten())
            };
            let whole = m
                .connector
                .as_ref()
                .is_some_and(|c| c.reads_whole(&m.remote));
            let fast = skwir_core::fs::reach::proved_fast(&m.remote);
            known.push(Known {
                point: point.clone(),
                address: address.map(Address::location),
                label: m.recognised.label.clone(),
                thumbnails: thumbnails(rule, near, whole, fast),
                forget: m.connector.as_ref().and_then(|c| c.forget(&m.remote)),
                trash: state.features.get(point).and_then(|f| f.trash.clone()),
                // Asked when the window comes back: how full it is, and what
                // other programs left to be sent there (rclone's, ADR-102).
                looked_at: Some({
                    let (this, weak, point) = (self.clone(), app.weak(), point.clone());
                    let uploads = m.remote.fstype == rclone::FSTYPE;
                    skwir_core::fs::reach::Look(Arc::new(move || {
                        let Some(app) = weak.upgrade() else {
                            return;
                        };
                        this.ask_again(&app, |at| at == point);
                        if uploads {
                            this.look_at_uploads(&app, &point);
                        }
                    }))
                }),
            });
            let mut place = match m.recognised.device {
                Some(device) => Place::new(
                    DEVICES,
                    m.recognised.label.clone(),
                    device.icon(),
                    Location::from_path(point),
                ),
                None => Place::new(
                    GROUP,
                    m.recognised.label.clone(),
                    "folder-remote",
                    Location::from_path(point),
                ),
            };
            place.key = Some(format!("remote:{key}"));
            if m.ejects {
                place.eject = Some(
                    Invocation::new("remote.disconnect")
                        .with("path", PathArg::from(point.as_path())),
                );
            }
            if state.lost.contains(point) {
                place.unavailable = Some(words!(
                    remote_lost = "The connection is lost: open it to connect again"
                ));
                if let Some(address) = address {
                    place.activate = Some(Invocation::navigate(&address.location()));
                }
            }
            place.busy = state
                .busy
                .get(&point.display().to_string())
                .cloned()
                .or_else(|| {
                    state
                        .waiting
                        .get(point)
                        .filter(|files| **files > 0)
                        .map(|files| sending(*files))
                });
            place.space = state.space.get(point).copied();
            place.menu = thumbnail_items(&key, rule);
            place
                .menu
                .extend(address.and_then(|a| forget_item(a, &remembered)));
            place.menu.extend(address.and_then(options_item));
            if let Some(address) = address {
                place
                    .menu
                    .extend(configure::server_items(address, &state.at_login));
            }
            if in_folder[at] {
                of_servers[at] = Some(place.clone());
            }
            if gives_way(at) {
                continue;
            }
            if let Some(name) = address.and_then(configure::remote_name) {
                let kind = state
                    .servers
                    .iter()
                    .find(|s| s.address.host == name && s.address.scheme == Scheme::Rclone)
                    .map(|s| s.kind.as_str());
                // Mounted, but not of the configuration (a connection
                // string's): nothing to edit.
                if let Some(kind) = kind {
                    place.menu.extend(configure::menu_items(
                        name,
                        configure::signs_in(kind),
                        unfinished.iter().any(|u| u == name),
                    ));
                }
            } else if let Some(kept) = at_top[at]
                .map(|server| &state.servers[server])
                .filter(|server| server.kept.by_address())
            {
                // A server kept at the mount's top: its own Edit and
                // Remove, on its mount's place alone.
                place.menu.extend(configure::kept_items(&kept.address));
            }
            places.push(place);
        }
        // The devices plugged in and not mounted (ADR-096), as unmounted
        // disks are: the same place mounted or not, mounted once opened;
        // one two tools offer (GVfs, KDE's kmtpd: ADR-097) listed once.
        let mut listed: Vec<&Address> = Vec::new();
        for offered in state.devices.values().flatten() {
            if listed.iter().any(|a| a.same_server(&offered.address)) {
                continue;
            }
            listed.push(&offered.address);
            let shown = mounted.iter().any(|m| {
                m.recognised
                    .address
                    .as_ref()
                    .is_some_and(|a| a.same_server(&offered.address))
            });
            if shown {
                continue;
            }
            let mut place = Place::new(
                DEVICES,
                offered.label(),
                offered.device.icon(),
                offered.address.location(),
            );
            place.key = Some(format!("remote:{}", offered.address));
            place.unavailable = Some(words!(
                remote_device_not_mounted = "Not mounted. Click to mount and open it."
            ));
            places.push(place);
        }
        for (server, held) in state.servers.iter().zip(&held) {
            match held {
                // Drawn as the mount's own place.
                Some((_, [])) => continue,
                Some((at, below)) => {
                    if let Some(place) = of_servers[*at].clone() {
                        let answered = state.answered.get(&server.address.to_string());
                        places.push(as_mounted(
                            place,
                            server,
                            &mounted[*at],
                            below,
                            answered.map(PathBuf::as_path),
                            &unfinished,
                        ));
                    }
                    continue;
                }
                None => {}
            }
            let location = server.address.location();
            let mut place = Place::new(GROUP, server.label.clone(), "network-server", location);
            place.key = Some(format!("remote:{}", server.address));
            place.menu.extend(forget_item(&server.address, &remembered));
            place.menu.extend(options_item(&server.address));
            place
                .menu
                .extend(configure::server_items(&server.address, &state.at_login));
            place.menu.extend(own_items(server, &unfinished));
            // Drawn as not ready, saying why, opened all the same.
            if let Some(name) = configure::remote_name(&server.address)
                && unfinished.iter().any(|u| u == name)
            {
                place.unavailable = Some(words!(
                    remote_half_made =
                        "{ $name } is not finished setting up: Finish Setting Up… in its menu",
                    name = name,
                    note = "name: a remote of rclone's, as its configuration names it. \
                            Finish Setting Up… is the item of its menu."
                ));
            }
            places.push(place);
        }
        // The servers announced on the local network, found while it is
        // shown (ADR-084): where Avahi and wsdd run.
        if cfg!(target_os = "linux") {
            let mut place = Place::new(
                GROUP,
                words!(
                    remote_nearby_title = "Nearby",
                    noun,
                    note = "The place that lists the servers found on this network."
                ),
                "network-workgroup",
                nearby::location(),
            );
            place.key = Some("remote:nearby".into());
            places.push(place);
        }
        state.proved_seen = proved;
        let points: Vec<PathBuf> = known.iter().map(|k| k.point.clone()).collect();
        let moved = points != state.known_points;
        state.known_points = points;
        drop(state);
        app.registry.places.replace_owner(ID, places);
        app.registry.remotes.replace_owner(ID, known);
        app.registry.reopens.replace_owner_if_changed(ID, reopens);
        // A bridge's folder mounted or let go of, which no table says
        // (ADR-149).
        if moved {
            app.volumes_changed();
        }
        app.wake();
    }
}

/// The mount that holds each of `servers`, by its index in `mounted`, and
/// where in it the server's folder is: the first mount whose address the
/// server's is below (`sftp://nas/srv/photos` in the mount of
/// `sftp://me@nas/`, at `srv/photos`). `None` where none holds it.
fn held<'a>(servers: &'a [Server], mounted: &[Mounted]) -> Vec<Option<(usize, &'a [u8])>> {
    servers
        .iter()
        .map(|server| {
            mounted.iter().enumerate().find_map(|(at, m)| {
                let below = m.recognised.address.as_ref()?.below(&server.address)?;
                Some((at, below))
            })
        })
        .collect()
}

/// What the servers `held` (`held`) make of each mount: whether one is
/// kept with a folder below its top, and the first held at its top, by its
/// index among the servers. A mount gives way to its servers' places only
/// where they are all of the first kind and it is no device's.
struct Holds {
    in_folder: Vec<bool>,
    at_top: Vec<Option<usize>>,
}

fn holds(held: &[Option<(usize, &[u8])>], mounted: &[Mounted]) -> Holds {
    let mut in_folder = vec![false; mounted.len()];
    let mut at_top: Vec<Option<usize>> = vec![None; mounted.len()];
    for (server, held) in held.iter().enumerate() {
        match held {
            Some((at, [])) => {
                at_top[*at] = at_top[*at].or(Some(server));
            }
            Some((at, _)) => in_folder[*at] = true,
            None => {}
        }
    }
    Holds { in_folder, at_top }
}

/// `server`'s place while `m` holds it, at `below`, a folder below the
/// mount's top: the mount's `place` (its eject, its space, its
/// thumbnails, the connection's items) under the server's name and key,
/// opening the server's own folder. A server is one place mounted or not
/// (`docs/invariants/remote.md`): what the user named, moved or hid it as
/// holds, in its group, and it never opens the mount's top. A device's
/// mount (a phone's) stays the device's place: the server's is drawn
/// beside it, among the servers. A tool that mounts each folder once
/// asked (kio-fuse) holds this one where it `answered`, below the mount;
/// until then it is opened by its address, which asks for it.
fn as_mounted(
    mut place: Place,
    server: &Server,
    m: &Mounted,
    below: &[u8],
    answered: Option<&Path>,
    unfinished: &[String],
) -> Place {
    place.title = server.label.clone();
    place.key = Some(format!("remote:{}", server.address));
    if m.recognised.device.is_some() {
        GROUP.clone_into(&mut place.group);
        place.icon = Some("folder-remote".to_owned());
    }
    let each_path = m.connector.as_ref().is_some_and(|c| c.mounts_each_path());
    let folder = join(&m.remote.point, below);
    place.location = match answered {
        _ if !each_path || below.is_empty() => Location::from_path(folder),
        Some(path) if path.starts_with(&m.remote.point) => Location::from_path(path),
        _ => server.address.location(),
    };
    // Connected again where it was lost: in its own folder.
    if place.activate.is_some() {
        place.activate = Some(Invocation::navigate(&server.address.location()));
    }
    place.menu.extend(own_items(server, unfinished));
    place
}

/// A server's own items in its place's menu, mounted or not: an rclone
/// remote's (Edit, Test, Finish Setting Up… while `unfinished` names it),
/// one added by its address its Edit and Remove.
fn own_items(server: &Server, unfinished: &[String]) -> Vec<skwir_core::registry::MenuItem> {
    if let Some(name) = configure::remote_name(&server.address) {
        configure::menu_items(
            name,
            configure::signs_in(&server.kind),
            unfinished.iter().any(|u| u == name),
        )
    } else if server.kept.by_address() {
        configure::kept_items(&server.address)
    } else {
        Vec::new()
    }
}

/// Whether the user may let go of a mount: Skwir's, GVfs's, one fstab lets
/// them mount; never kio-fuse's, which lets go of no server. Reads fstab
/// (`fstab::user_mountable`): as the mount is recognised, never under the
/// state's lock.
fn can_disconnect(connector: &dyn Connector, remote: &Remote, recognised: &Recognised) -> bool {
    match connector.id() {
        "fstab" => fstab::user_mountable(&remote.point),
        "sshfs" | "rclone" => recognised.ours || cfg!(target_os = "linux"),
        "kio" => false,
        _ => true,
    }
}

/// What a remote's thumbnails follow: the rule chosen (`never`, `small`,
/// `always`, `automatic`), Automatic being Always on the local network and
/// Never where that is not known (ADR-077); Small files near where its
/// program reads a file `whole` before any part of it (ADR-081), and
/// elsewhere once reads the user made proved it `fast` this session
/// (`fs::reach::note_read`).
fn thumbnails(rule: &str, near: Option<bool>, whole: bool, fast: Option<bool>) -> Thumbnails {
    match rule {
        "never" => Thumbnails::Never,
        "small" => Thumbnails::Small,
        "always" => Thumbnails::Always,
        _ => match (near, fast) {
            (Some(true), _) if whole => Thumbnails::Small,
            (Some(true), _) => Thumbnails::Always,
            // Farther, or not known to be near: Small files once reads
            // the user made proved it fast this session.
            (_, Some(true)) => Thumbnails::Small,
            _ => Thumbnails::Never,
        },
    }
}

/// The place's menu: its thumbnails, as chosen for it.
fn thumbnail_items(key: &str, rule: &str) -> Vec<skwir_core::registry::MenuItem> {
    let named = words!(
        remote_thumbnails_choice = "Thumbnails",
        note = "A place's menu: when its files show thumbnails."
    );
    [
        (
            "automatic",
            words!(remote_thumbnails_menu_automatic = "Automatic"),
        ),
        ("never", words!(remote_thumbnails_menu_never = "Never")),
        (
            "small",
            words!(remote_thumbnails_menu_small = "Small Files"),
        ),
        ("always", words!(remote_thumbnails_menu_always = "Always")),
    ]
    .into_iter()
    .zip(500..)
    .map(|((value, title), order)| {
        let mut item = skwir_core::registry::MenuItem::new(
            "place",
            title,
            "",
            order,
            Invocation::new("remote.thumbnails")
                .with("remote", key)
                .with("rule", value),
        )
        // In words: rows, the submenu's own row saying the rule set.
        .value_of(
            "remote.thumbnails",
            named.clone(),
            skwir_core::registry::ChoiceLook::Icons,
            value == rule,
        );
        item.submenu = Some(skwir_core::registry::Submenu::new(
            "remote.thumbnails",
            words!(
                remote_thumbnails_submenu = "Thumbnails",
                note = "A place's menu: the row of the submenu choosing when its files show thumbnails."
            ),
        ));
        item
    })
    .collect()
}

/// The menu item that forgets the password Skwir keeps for `address`'s
/// server, where it keeps one (`remembered`, ADR-082).
fn forget_item(address: &Address, remembered: &[String]) -> Option<skwir_core::registry::MenuItem> {
    let key = passwords::key(address, passwords::user_of(address).as_deref());
    remembered.contains(&key).then(|| {
        skwir_core::registry::MenuItem::new(
            "place",
            words!(remote_forget_password_menu = "Forget Password"),
            "",
            530,
            Invocation::new("remote.forget_password").with("server", key),
        )
    })
}

/// The menu item that opens the mount options of `address`'s server,
/// where a tool that mounts it takes some (ADR-086).
fn options_item(address: &Address) -> Option<skwir_core::registry::MenuItem> {
    options::takes_options(address.scheme).then(|| {
        skwir_core::registry::MenuItem::new(
            "place",
            words!(remote_mount_options_menu = "Mount Options…"),
            "sliders",
            420,
            Invocation::new("remote.mount_options").with("address", address.top().to_string()),
        )
    })
}

/// The servers whose password Skwir keeps in the keyring, by their key
/// (`passwords::key`, ADR-082).
pub(crate) fn remembered(app: &App) -> Vec<String> {
    app.settings.get(REMEMBERED).unwrap_or_default()
}

impl Remotes {
    /// Note in the plugin's data that the server keyed in `noted` has a
    /// password of Skwir's kept, or no longer (ADR-082): its key alone.
    /// Settings' list is told: a server's line forgets it.
    pub(crate) fn note_remembered(&self, app: &App, noted: &passwords::Noted) {
        note_remembered(app, noted);
        self.records.changed();
    }
}

/// [`Remotes::note_remembered`], the list unsaid.
fn note_remembered(app: &App, noted: &passwords::Noted) {
    let mut remembered = remembered(app);
    match noted {
        passwords::Noted::Kept(key) if !remembered.contains(key) => remembered.push(key.clone()),
        passwords::Noted::Forgot(key) => remembered.retain(|k| k != key),
        passwords::Noted::Kept(_) => return,
        passwords::Noted::NotKept(why) => {
            app.notify(
                NotifyLevel::Warning,
                words!(
                    remote_password_not_kept = "The password was not remembered: { $why }",
                    why = why.clone(),
                    note = "why: what the keyring said (No keyring answered)."
                ),
            );
            return;
        }
    }
    app.settings.set(
        REMEMBERED,
        toml::Value::Array(remembered.into_iter().map(toml::Value::String).collect()),
    );
    if let Err(error) = app.settings.save() {
        tracing::warn!("which servers have a kept password was not saved: {error:#}");
    }
}

/// Whether `error` says a FUSE program is gone (`ENOTCONN`).
fn lost(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotConnected
}

/// Whether the mount `remote` reaches no server: its program gone
/// (`lost`), or, for sshfs, its connection cut and not made again. sshfs
/// keeps its mount when its server dies, its `reconnect` failing at each
/// call with an input/output error (ADR-149, amended): asked first what it
/// never answers from its cache, how full the server is
/// (`statvfs@openssh.com`), then its root listed, as a server without that
/// extension is answered from memory. Elsewhere an input/output error says
/// a server refused something, not that its connection went: a stat, as
/// before (and on every system but Linux). Reads the mount: in its lane.
fn cut_off(remote: &reach::Remote) -> bool {
    let point = &remote.point;
    #[cfg(target_os = "linux")]
    if remote.fstype == sshfs::FSTYPE {
        use rustix::io::Errno;
        let cut = |error: Errno| matches!(error, Errno::IO | Errno::NOTCONN);
        let errno = |error: &std::io::Error| error.raw_os_error().map(Errno::from_raw_os_error);
        if skwir_core::fs::volume::answers(point).is_err_and(|error| errno(&error).is_some_and(cut))
        {
            return true;
        }
        let listed = std::fs::read_dir(point).and_then(|mut entries| entries.next().transpose());
        return listed.is_err_and(|error| errno(&error).is_some_and(cut));
    }
    std::fs::symlink_metadata(point).is_err_and(|error| lost(&error))
}

/// `point` joined with `below`, the bytes of a server's path: name by
/// name, never above `point` (an address's path holds no `..`, but a
/// mount's address read from elsewhere might), so what it leads to is
/// the mount's.
fn join(point: &Path, below: &[u8]) -> PathBuf {
    let below = PathBuf::from(skwir_core::os_path::from_bytes(below.to_vec()));
    let mut path = point.to_path_buf();
    let mut depth = 0usize;
    for part in below.components() {
        match part {
            std::path::Component::Normal(name) => {
                path.push(name);
                depth += 1;
            }
            std::path::Component::ParentDir if depth > 0 => {
                path.pop();
                depth -= 1;
            }
            _ => {}
        }
    }
    path
}

/// The plugin.
#[derive(Default)]
pub struct RemotePlugin {
    remotes: Remotes,
}

impl RemotePlugin {
    pub fn with(remotes: Remotes) -> Self {
        RemotePlugin { remotes }
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ConnectArgs {
    /// The address: `sftp://me@example.com/home/me`, `smb://nas/photos`,
    /// `rclone://gdrive/`.
    address: String,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
    /// Open it in a tab behind the one shown.
    #[serde(default)]
    tab: bool,
    /// A folder below the address to open, by the names leading to it:
    /// the one a pane could not show once its server was disconnected
    /// (ADR-149), opened in place of that step.
    #[serde(default)]
    below: Option<PathArg>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DisconnectArgs {
    /// The mount's folder, as the sidebar names it.
    path: PathArg,
    /// Let go of it even if a program uses it or the server does not
    /// answer (what still uses it sees it gone).
    #[serde(default)]
    force: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ForgetPasswordArgs {
    /// The server, by its key (`smb://WORK;me@nas`), as its menu names it.
    server: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ThumbnailsArgs {
    /// The remote: its address, or its folder for one without.
    remote: String,
    /// `automatic`, `never`, `small` or `always`.
    rule: String,
}

/// The cache of the app the plugin was registered in: what its connectors
/// keep beyond a connection (what rclone listed, a phone's files) stays in
/// that app's own, and they are built with no `App` at hand. Before any
/// registration, the system's.
static CACHE: Mutex<Option<skwir_core::cache::Cache>> = Mutex::new(None);

fn cache() -> skwir_core::cache::Cache {
    use skwir_core::cache::{Cache, LAYOUT};
    CACHE
        .lock()
        .clone()
        .unwrap_or_else(|| Cache::new(Cache::default_root().join(LAYOUT.to_string())))
}

impl Plugin for RemotePlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(remote_plugin_title = "Remote folders")
    }

    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            remote_plugin_description = "Open SFTP, SMB, WebDAV, FTP, NFS and rclone addresses as folders, through sshfs, GVfs, rclone or the system's mounts; phones and cameras through GVfs"
        )
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Browsing
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Network, Capability::Programs])
    }

    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        use skwir_core::plugin::Requirement;
        vec![
            Requirement::program(
                "sshfs",
                words!(remote_needs_sshfs = "Opening SFTP addresses through your ssh"),
            ),
            Requirement::program(
                "gio",
                words!(remote_needs_gio = "Opening addresses through GVfs, the desktop's remotes"),
            ),
            Requirement::program(
                "rclone",
                words!(remote_needs_rclone = "Opening rclone's remotes and other addresses"),
            ),
            kmtp::WORKER.requirement(words!(
                remote_needs_kmtp = "Opening a phone KDE's kmtpd holds"
            )),
        ]
    }

    fn on_disable(&self, _app: &App) {
        let mut state = self.remotes.state.lock();
        if let Some(lifetime) = state.lifetime.take() {
            lifetime.cancel();
        }
        state.devices.clear();
        // Its watcher's thread ends with it.
        state.sources = None;
        // Off, what it knew of the bridge's folders is withdrawn, and the
        // core no longer counts them (ADR-149, 12): on again, each is told
        // anew, so that one let go of later is told so.
        state.known_points.clear();
    }

    fn on_looked_at(&self, app: &App) {
        self.remotes.looked_at(app);
    }

    fn register(&self, app: &App) {
        *CACHE.lock() = Some(app.cache.clone());
        for transfer in self.remotes.connectors.iter().flat_map(|c| c.transfers()) {
            app.transfers.register(ID, transfer);
        }
        // A copy down that proved a server fast, or slow again, changes
        // its thumbnails' Automatic: published again at its plan's end.
        // The mounts it wrote to or freed room on are asked how full they
        // are.
        let (remotes, weak) = (self.remotes.clone(), app.weak());
        app.executor.observe(
            ID,
            Arc::new(move |changes| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let (seen, mounted) = {
                    let state = remotes.state.lock();
                    (state.proved_seen, Arc::clone(&state.mounted))
                };
                if skwir_core::fs::reach::proved_changed() != seen {
                    remotes.publish(&app);
                }
                let touched = touched(
                    mounted.iter().map(|m| m.remote.point.as_path()).collect(),
                    changes,
                );
                if !touched.is_empty() {
                    remotes.ask_again(&app, |point| touched.iter().any(|t| t == point));
                }
            }),
        );
        if let Some(previous) = self
            .remotes
            .state
            .lock()
            .lifetime
            .replace(app.lifetime().child())
        {
            previous.cancel();
        }
        // What the Network section shows is followed, whoever changes it
        // (ADR-077, ADR-086, amended 2026-11-15): the table of mounts, and
        // the files the servers are read from, once read.
        if let Some(lifetime) = self.remotes.lifetime() {
            let (remotes, weak) = (self.remotes.clone(), app.weak());
            reach::follow(&lifetime, move || {
                if let Some(app) = weak.upgrade() {
                    remotes.mounts_told(&app);
                }
            });
            let (remotes, weak) = (self.remotes.clone(), app.weak());
            let sources = sources::Sources::start(move || {
                let Some(app) = weak.upgrade() else {
                    return false;
                };
                if lifetime.is_cancelled() {
                    return false;
                }
                remotes.sources_told(&app);
                true
            });
            self.remotes.state.lock().sources =
                sources.map(|sources| Arc::new(Mutex::new(sources)));
        }
        app.settings.declare(
            SettingSpec::text(
                CONNECTORS,
                words!(remote_connectors = "Tools that open addresses"),
                (
                    words!(
                        remote_connectors_description = "The tools that mount an sftp://, smb:// or other address, tried in this order among those installed: sshfs, kmtp (a phone KDE holds), gvfs, kio, rclone, fstab.",
                        note = "sshfs, kmtp, gvfs, kio, rclone and fstab are what the setting's value names: never translated."
                    ),
                    words!(
                        remote_connectors_description_more = "On KDE's desktop, kio comes before gvfs.",
                        note = "sshfs, kmtp, gvfs, kio, rclone and fstab are what the setting's value names: never translated."
                    ),
                ),
                default_order(),
            )
            .page(skwir_core::settings::page::NETWORK)
            .section(skwir_core::settings::section::OPENING_ADDRESSES),
        );
        app.settings.declare(
            SettingSpec::choice_words(
                THUMBNAILS,
                words!(remote_thumbnails = "Thumbnails in remote folders"),
                words!(
                    remote_thumbnails_description = "For a remote you chose nothing for in its menu: Automatic shows them on the local network only, as reading every picture across the internet would be slow."
                ),
                "automatic",
                vec![
                    ("automatic", words!(remote_thumbnails_automatic = "Automatic")),
                    ("never", words!(remote_thumbnails_never = "Never")),
                    ("small", words!(remote_thumbnails_small = "Small files")),
                    ("always", words!(remote_thumbnails_always = "Always")),
                ],
            )
            .page(skwir_core::settings::page::VIEWS)
            .section(skwir_core::settings::section::THUMBNAILS),
        );
        app.settings.declare(SettingSpec::data(
            CHOSEN,
            words!(remote_chosen_data = "The thumbnails chosen for each remote, by its address"),
            toml::Value::Table(toml::map::Map::new()),
        ));
        app.settings.declare(SettingSpec::data(
            options::SETTING,
            words!(
                remote_options_data = "The mount options chosen for each server, by its address"
            ),
            toml::Value::Table(toml::map::Map::new()),
        ));
        app.settings.declare(SettingSpec::data(
            rclone_forms::UNFINISHED,
            words!(
                remote_unfinished_data =
                    "The remotes of rclone's Skwir began to set up and did not finish, by name"
            ),
            toml::Value::Array(Vec::new()),
        ));
        app.settings.declare(SettingSpec::data(
            REMEMBERED,
            words!(
                remote_remembered_data =
                    "The servers whose password is kept in the keyring, by their address"
            ),
            toml::Value::Array(Vec::new()),
        ));
        for scheme in Scheme::ALL {
            app.register_provider(
                ID,
                Arc::new(provider::Provider::new(
                    scheme.name(),
                    self.remotes.clone(),
                    app,
                )),
            );
        }
        for (alias, _) in Scheme::ALIASES {
            app.register_provider(
                ID,
                Arc::new(provider::Provider::new(alias, self.remotes.clone(), app)),
            );
        }
        // Servers found on the local network, while shown (ADR-084).
        app.register_provider(ID, Arc::new(nearby::Provider));
        // The servers the tools keep, changed from Skwir (ADR-086).
        configure::register(&self.remotes, app);
        // Windows lists no mount table: rclone's mounts are the folders of
        // Skwir's that WinFsp made mount points.
        #[cfg(windows)]
        if let Some(folder) = connector::mounts_folder() {
            reach::mounts_in(folder, rclone::FSTYPE);
        }
        // The devices plugged in that a tool would mount, followed while
        // the plugin is enabled (ADR-096, ADR-097).
        if let Some(lifetime) = self.remotes.lifetime() {
            self.remotes.follow_devices(app, &lifetime);
        }
        let remotes = self.remotes.clone();
        app.commands.register::<ConnectArgs, _>(
            ID, Topic::Places,
            "remote.connect",
            words!(remote_connect = "Connect to server"),
            words!(
                remote_connect_description = "Open a remote folder by its address (sftp://, smb://, dav://, ftp://, nfs://, afp://, rclone://)"
            ),
            move |args, app| {
                let address = Address::parse(args.address.trim())?;
                let _ = &remotes;
                let pane = app.pane_or_active(args.pane);
                let below = args.below.map(PathArg::into_inner);
                let location = match &below {
                    Some(below) => skwir_core::remotes::address_below(
                        &address.location(),
                        skwir_core::registry::Reopen::below(below)?,
                    ),
                    None => address.location(),
                };
                if args.tab {
                    app.new_tab(Some(pane), Some(location), false)?;
                } else if below.is_some() {
                    // In place of the step its folder could not be shown in.
                    app.navigate_in_place(pane, &location)?;
                } else {
                    app.navigate(pane, &location)?;
                }
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<DisconnectArgs, _>(
            ID,
            Topic::Places,
            "remote.disconnect",
            words!(remote_disconnect = "Disconnect"),
            words!(
                remote_disconnect_description =
                    "Let go of a remote folder: what shows it says so, and shows it again once connected"
            ),
            move |args, app| {
                remotes.disconnect(app, &args.path.into_inner(), args.force)?;
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<ThumbnailsArgs, _>(
            ID, Topic::Places,
            "remote.thumbnails",
            words!(remote_thumbnails_of = "Thumbnails of a remote"),
            words!(
                remote_thumbnails_of_description = "Choose when a remote folder's files show thumbnails: automatic, never, small or always",
                note = "automatic, never, small and always are the rules the command takes: never translated."
            ),
            move |args, app| {
                anyhow::ensure!(
                    ["automatic", "never", "small", "always"].contains(&args.rule.as_str()),
                    words!(
                        remote_thumbnails_rule = "a rule is automatic, never, small or always",
                        note = "automatic, never, small and always are the rules the command takes: never translated."
                    )
                );
                let (remotes, weak) = (remotes.clone(), app.weak());
                // Kept in the settings file: written on a worker from the
                // window's thread, else before answering (a `skwir cmd`
                // without a window ends once answered).
                let save = move || {
                    let Some(app) = weak.upgrade() else { return };
                    let mut chosen: toml::map::Map<String, toml::Value> =
                        app.settings.get(CHOSEN).unwrap_or_default();
                    if args.rule == "automatic" {
                        chosen.remove(&args.remote);
                    } else {
                        chosen.insert(args.remote, toml::Value::String(args.rule));
                    }
                    app.settings.set(CHOSEN, toml::Value::Table(chosen));
                    if let Err(error) = app.settings.save() {
                        app.notify(
                            NotifyLevel::Error,
                            words!(
                                remote_thumbnails_not_saved = "The choice of when the remote's files show thumbnails was not saved: { $error }",
                                error = error,
                                note = "An error: the choice made in a remote's Thumbnails menu (automatic, never, small files, always) was not written to the settings."
                            ),
                        );
                    }
                    remotes.publish(&app);
                };
                if skwir_core::scheduler::on_window_thread() {
                    app.scheduler.spawn(
                        skwir_core::scheduler::Priority::Normal,
                        "remote.thumbnails",
                        app.lifetime().child(),
                        save,
                    );
                } else {
                    save();
                }
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<ForgetPasswordArgs, _>(
            ID, Topic::Places,
            "remote.forget_password",
            words!(remote_forget_password = "Forget password"),
            words!(
                remote_forget_password_description = "Remove from the keyring the password Skwir keeps for a server (never one another program kept)"
            ),
            move |args, app| {
                let remembered: Vec<String> = app.settings.get(REMEMBERED).unwrap_or_default();
                anyhow::ensure!(
                    remembered.contains(&args.server),
                    words!(
                        remote_no_password_kept = "Skwir keeps no password for { $server }",
                        server = args.server.clone(),
                        note = "server: the server as the keyring knows it (smb://me@nas)."
                    )
                );
                let (remotes, weak) = (remotes.clone(), app.weak());
                // The keyring may wait on its own window: on a worker.
                app.scheduler.spawn(
                    skwir_core::scheduler::Priority::Normal,
                    "remote.forget_password",
                    app.lifetime().child(),
                    move || {
                        let Some(app) = weak.upgrade() else { return };
                        match remotes.keyring.forget(&args.server) {
                            Ok(()) => {
                                remotes.note_remembered(&app, &passwords::Noted::Forgot(args.server));
                                app.notify(
                                    NotifyLevel::Info,
                                    words!(remote_password_forgotten = "The password is forgotten"),
                                );
                            }
                            Err(error) => app.notify(
                                NotifyLevel::Error,
                                words!(
                                    remote_password_not_forgotten = "The password was not forgotten: { $error }",
                                    error = error
                                ),
                            ),
                        }
                        remotes.publish(&app);
                    },
                );
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands
            .register::<skwir_core::command::NoArgs, _>(
                ID,
                Topic::Places,
                "remote.refresh",
                words!(remote_refresh = "Refresh remote folders"),
                words!(
                    remote_refresh_description =
                        "Read again what is mounted and which servers the tools know"
                ),
                move |_, app| {
                    reach::changed();
                    rclone::forget_listed();
                    remotes.refresh(app);
                    Ok(Outcome::Done)
                },
            )
            .shows();
        // What is known already is in the sidebar at once (Nearby, and the
        // mounts and servers read before it was turned off): the refresh
        // that reads the servers again runs on a thread of its own (AR-11).
        self.remotes.publish(app);
        self.remotes.refresh(app);
    }

    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys
            .iter()
            .any(|k| k == THUMBNAILS || k == CHOSEN || k == REMEMBERED)
        {
            // What these change is worded by `publish` alone.
            self.remotes.publish(app);
        }
    }
}

impl Remotes {
    /// The addresses known: those mounted, the servers the tools name, the
    /// remote bookmarks. Lists GVfs's bridge and reads the bookmarks: on a
    /// worker.
    fn known_addresses(&self) -> Vec<Address> {
        let mut known: Vec<Address> = self
            .mounted()
            .into_iter()
            .filter_map(|m| m.recognised.address)
            .collect();
        {
            let state = self.state.lock();
            known.extend(state.servers.iter().map(|s| s.address.clone()));
            known.extend(state.devices.values().flatten().map(|o| o.address.clone()));
        }
        known.extend(
            skwir_platform::places::remote_bookmarks()
                .into_iter()
                .filter_map(|b| Address::parse(&b.address).ok()),
        );
        known
    }

    /// Ask the rclone mount Skwir made at `point` what other programs left
    /// waiting to be sent there (ADR-102), and while some waits, say so on
    /// its place and ask again every `UPLOADS_LOOKED_EVERY`, on a thread of
    /// its own, until none does or the mount is gone. In the remote's lane
    /// (`Known::looked_at`): its remote control may wait.
    pub fn look_at_uploads(&self, app: &App, point: &Path) {
        let Some(at) = uploads::control_of(point) else {
            return;
        };
        let Some(lifetime) = self.lifetime() else {
            return;
        };
        let files = uploads::waiting(&at, &lifetime).unwrap_or(0);
        {
            let mut state = self.state.lock();
            if files == 0 || !state.watching.insert(point.to_path_buf()) {
                return;
            }
            state.waiting.insert(point.to_path_buf(), files);
        }
        self.publish(app);
        let (this, weak, followed) = (self.clone(), app.weak(), point.to_path_buf());
        let watched = std::thread::Builder::new()
            .name("skwir-uploads".into())
            .spawn(move || {
                let mut said = files;
                loop {
                    std::thread::sleep(UPLOADS_LOOKED_EVERY);
                    let now = if lifetime.is_cancelled() {
                        0
                    } else {
                        uploads::control_of(&followed)
                            .and_then(|at| uploads::waiting(&at, &lifetime).ok())
                            .unwrap_or(0)
                    };
                    let Some(app) = weak.upgrade() else {
                        return;
                    };
                    if now == 0 {
                        let mut state = this.state.lock();
                        state.waiting.remove(&followed);
                        state.watching.remove(&followed);
                    } else {
                        this.state.lock().waiting.insert(followed.clone(), now);
                    }
                    // Said on the mount's place: nothing else is read.
                    if now != said {
                        this.publish(&app);
                        said = now;
                    }
                    if now == 0 {
                        return;
                    }
                }
            });
        if let Err(error) = watched {
            tracing::debug!("what waits to be sent is not followed: {error}");
            self.state.lock().watching.remove(point);
        }
    }

    /// Let go of the mount at `point`: what shows it goes home, then its
    /// tool lets go of it, on a worker, saying how it went. Refused while
    /// an action of Skwir's is under way there, unless `force`d; what else
    /// Skwir still reads there it gave up on, and is cut
    /// (`mounter::when_busy`).
    fn disconnect(&self, app: &App, point: &Path, force: bool) -> anyhow::Result<()> {
        let (label, letting_go) = self.begin_disconnect(app, point, force)?;
        let weak = app.weak();
        std::thread::Builder::new()
            .name("skwir-disconnect".into())
            .spawn(move || {
                let result = letting_go();
                if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                    app.notify(NotifyLevel::Error, not_disconnected(&label, error));
                }
            })?;
        Ok(())
    }

    /// [`Remotes::disconnect`], here: once it is let go of, or why not,
    /// which the caller says. Blocks: on a worker.
    fn disconnect_here(&self, app: &App, point: &Path, force: bool) -> anyhow::Result<()> {
        let (_, letting_go) = self.begin_disconnect(app, point, force)?;
        letting_go()
    }

    /// What [`Remotes::disconnect`] does before letting go: the mount's
    /// name and the letting go, which says so once done.
    fn begin_disconnect(
        &self,
        app: &App,
        point: &Path,
        force: bool,
    ) -> anyhow::Result<(Words, impl FnOnce() -> anyhow::Result<()> + Send + 'static)> {
        let mounted = self
            .mounted()
            .into_iter()
            .find(|m| m.remote.point == point)
            .ok_or_else(|| {
                anyhow::anyhow!(words!(
                    remote_not_mounted_there = "no remote folder is mounted there"
                ))
            })?;
        let connector = mounted.connector.clone().ok_or_else(|| {
            anyhow::anyhow!(words!(
                remote_tool_unknown = "Skwir does not know the tool that mounted it"
            ))
        })?;
        let label = mounted.recognised.label.clone();
        if !force {
            under_way(app, point).map_err(|e| anyhow::anyhow!(not_disconnected(&label, e)))?;
        }
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/"));
        app.release(point, &Location::from_path(home))?;
        let key = point.display().to_string();
        self.state.lock().busy.insert(
            key.clone(),
            words!(
                remote_disconnecting = "Disconnecting…",
                note = "Said on a remote folder's place while it is let go of."
            ),
        );
        self.refresh(app);
        let this = self.clone();
        let lifetime = self.lifetime().unwrap_or_default();
        let weak = app.weak();
        let said = label.clone();
        let letting_go = move || {
            // What showed it lets go first: the folders left close on a
            // worker a moment later, and what reads it in its lane ends;
            // until then the system refuses the unmount as busy. Not a
            // read waiting for a server that does not answer: that one
            // waits for the server, not for time, and is cut.
            let mut result = Ok(());
            // Whether its server stopped answering: what waits there is
            // not waited for (it cannot be sent), but said.
            let mut stalled = false;
            if let Some(app) = weak.upgrade() {
                let (lane, until) = (mounted.remote.lane(), Instant::now() + LET_GO);
                loop {
                    let left = skwir_core::fs::wait_let_go(
                        &mounted.remote.point,
                        LOOK.min(until.saturating_duration_since(Instant::now())),
                    );
                    match app.scheduler.stalled(lane) {
                        None if left => break,
                        Some(ran) if ran >= skwir_core::fs::reach::NOT_ANSWERING => break,
                        _ if Instant::now() >= until => break,
                        _ => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
                // An action asked meanwhile.
                if !force {
                    result = under_way(&app, &mounted.remote.point);
                }
                stalled = app
                    .scheduler
                    .stalled(lane)
                    .is_some_and(|ran| ran >= skwir_core::fs::reach::NOT_ANSWERING);
            }
            // What waits there to be sent goes first (ADR-102): a refusal
            // keeps the mount, and what it keeps, connected.
            let control = (mounted.remote.fstype == rclone::FSTYPE)
                .then(|| uploads::control_of(&mounted.remote.point))
                .flatten();
            let sends = !force && !stalled;
            if let (Some(at), true) = (&control, sends) {
                result = result.and_then(|()| {
                    let mut left = |left: skwir_core::op::transfer::Left| {
                        if let Some(app) = weak.upgrade() {
                            this.state
                                .lock()
                                .busy
                                .insert(key.clone(), sending(left.files));
                            this.refresh(&app);
                        }
                    };
                    uploads::send_now(at, &lifetime, &|| {}, &mut left)
                        .map_err(|e| anyhow::anyhow!(not_disconnected(&said, e)))
                });
            }
            let kept = match (&control, sends) {
                (Some(at), false) => uploads::waiting(at, &lifetime).unwrap_or(0),
                _ => 0,
            };
            let mut result = result.and_then(|()| {
                connector.disconnect(&mounted.remote, &mounted.recognised, &lifetime)
            });
            if result.is_err() && force && !mounted.remote.by_kernel() {
                result =
                    connector.force_disconnect(&mounted.remote, &mounted.recognised, &lifetime);
            }
            this.state.lock().busy.remove(&key);
            if let Some(app) = weak.upgrade() {
                this.refresh(&app);
                match (&result, kept) {
                    (Ok(()), 0) => {
                        app.notify(
                            NotifyLevel::Info,
                            words!(
                                remote_disconnected = "Disconnected from { $name }",
                                name = crate::address::named(&said),
                                note = "name: a remote folder's, as its place names it."
                            ),
                        );
                    }
                    (Ok(()), kept) => app.notify(
                        NotifyLevel::Warning,
                        words!(
                            remote_disconnected_kept = "Disconnected from { $name }: { $count ->
                                [one] { $count } file
                               *[other] { $count } files
                            } not sent yet stay in rclone's cache, sent when it next connects",
                            name = crate::address::named(&said),
                            count = kept,
                            note = "name: a remote folder's, as its place names it; count: the \
                                    files other programs wrote there that its server has not \
                                    been sent yet."
                        ),
                    ),
                    (Err(_), _) => {}
                }
            }
            result
        };
        Ok((label, letting_go))
    }
}

/// How often a mount whose files wait to be sent is asked again, while
/// some do (`Remotes::look_at_uploads`): a test's at once.
const UPLOADS_LOOKED_EVERY: Duration = if cfg!(test) {
    Duration::from_millis(20)
} else {
    Duration::from_secs(2)
};

/// Which of the mount `points` `changes` name a path under: one pass over
/// the changes, ended once every mount is found (mounts are few, a plan's
/// changes may be a folder's 100,000).
fn touched(points: Vec<&Path>, changes: &[skwir_core::op::Change]) -> Vec<PathBuf> {
    let mut left = points;
    let mut touched = Vec::new();
    'changes: for change in changes {
        for path in change.paths() {
            if let Some(i) = left.iter().position(|point| path.starts_with(point)) {
                touched.push(left.swap_remove(i).to_path_buf());
                if left.is_empty() {
                    break 'changes;
                }
            }
        }
    }
    touched
}

/// What a mount's place says while it sends what waits.
fn sending(files: usize) -> Words {
    words!(
        remote_sending = "Sending { $count ->
            [one] { $count } file
           *[other] { $count } files
        } to the server…",
        count = files,
        note = "Said on a remote folder's place while what other programs wrote there is sent."
    )
}

/// The mount `label` names was not let go of, and why.
fn not_disconnected(label: &Words, error: anyhow::Error) -> Words {
    words!(
        remote_not_disconnected = "{ $name } was not disconnected: { $error }",
        name = crate::address::named(label),
        error = error,
        note = "name: a remote folder's, as its place names it."
    )
}

/// `items` as a list argument: “a”, “b” and “c”, joined as the reader's
/// language joins a list.
fn listed(items: Vec<Words>) -> skwir_core::words::Arg {
    skwir_core::words::List::and(items).into()
}

/// Refused while actions of Skwir's are under way at or below `point`
/// (`Executor::refuse_under_way`), named: letting go of the mount would
/// cut them.
fn under_way(app: &App, point: &Path) -> anyhow::Result<()> {
    app.executor.refuse_under_way(point)
}

/// How long a question waits between looks at whether its connection was
/// cancelled, and letting go of a mount between looks at what holds it.
const LOOK: Duration = Duration::from_millis(100);

/// How long letting go of a mount waits for what showed it and what read
/// it there to let go first.
const LET_GO: Duration = Duration::from_secs(10);

/// What one connector asks, passed on, noting whether it asked anything
/// (`Remotes::reach`): a tool that failed once the user answered it is not
/// followed by another asking again.
struct Heard {
    ask: Arc<dyn Ask>,
    asked: std::sync::atomic::AtomicBool,
}

impl Heard {
    fn asked(&self) -> bool {
        self.asked.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn noted(&self) -> &dyn Ask {
        self.asked.store(true, std::sync::atomic::Ordering::Relaxed);
        &*self.ask
    }
}

impl Ask for Heard {
    fn secret(&self, title: &Words, accept: &Words, cancel: &CancelToken) -> Option<String> {
        self.noted().secret(title, accept, cancel)
    }
    fn unprompted(&self, accept: &Words, cancel: &CancelToken) -> Option<String> {
        self.noted().unprompted(accept, cancel)
    }
    fn confirm(&self, title: &Words, detail: &Words, cancel: &CancelToken) -> bool {
        self.noted().confirm(title, detail, cancel)
    }
    fn password(&self, title: &Words, accept: &Words, cancel: &CancelToken) -> Option<Password> {
        self.noted().password(title, accept, cancel)
    }
    // Asked of no one.
    fn kept_password(&self) -> Option<String> {
        self.ask.kept_password()
    }
}

/// What a connection asks of the window: a password in its masked
/// prompt, a server it cannot vouch for as a question.
pub struct WindowAsk {
    pub app: std::sync::Weak<App>,
}

impl Ask for WindowAsk {
    fn secret(&self, title: &Words, accept: &Words, cancel: &CancelToken) -> Option<String> {
        self.asked(title, accept, false, cancel)
            .map(|answer| answer.text)
    }

    fn password(&self, title: &Words, accept: &Words, cancel: &CancelToken) -> Option<Password> {
        self.asked(title, accept, true, cancel)
            .map(|answer| Password {
                text: answer.text,
                remember: answer.remember,
            })
    }

    fn confirm(&self, title: &Words, detail: &Words, cancel: &CancelToken) -> bool {
        self.confirmed(title, detail, cancel)
    }
}

impl WindowAsk {
    /// Ask the window's masked prompt, offering Remember where `password`.
    fn asked(
        &self,
        title: &Words,
        accept: &Words,
        password: bool,
        cancel: &CancelToken,
    ) -> Option<skwir_core::app::SecretAnswer> {
        let app = self.app.upgrade()?;
        if !app.can_ask() {
            return None;
        }
        let (id, answer) = if password {
            app.ask_password(title.clone(), accept.clone())
        } else {
            app.ask_secret(title.clone(), accept.clone())
        };
        drop(app);
        loop {
            match answer.recv_timeout(LOOK) {
                Ok(answer) => return answer,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    if let Some(app) = self.app.upgrade() {
                        app.withdraw_secret(id);
                    }
                    return None;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn confirmed(&self, title: &Words, detail: &Words, cancel: &CancelToken) -> bool {
        let Some(app) = self.app.upgrade() else {
            return false;
        };
        if !app.can_ask() {
            return false;
        }
        let (id, answer) = app.ask_as(
            skwir_core::app::Asker::Connection,
            title.clone(),
            detail.clone(),
        );
        drop(app);
        loop {
            match answer.recv_timeout(LOOK) {
                Ok(answer) => return answer,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    if let Some(app) = self.app.upgrade() {
                        app.withdraw(id);
                    }
                    return false;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// What the connectors keep is in the cache of the app they serve,
    /// and an app with a cache of its own leaves the system's alone.
    #[test]
    fn connectors_keep_what_they_keep_in_the_app_s_own_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let app = super::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = super::RemotePlugin::with(super::Remotes::with(Vec::new()));
        app.plugins.add(std::sync::Arc::new(plugin), &app);
        assert_eq!(super::cache().root(), app.cache.root());
    }

    /// Automatic: Always near, Small files near where the program reads a
    /// file whole, and Small files elsewhere once reads proved the server
    /// fast this session; Never otherwise. A rule chosen is the rule.
    #[test]
    fn automatic_gives_small_files_where_reads_proved_a_server_fast() {
        use skwir_core::fs::reach::Thumbnails::{Always, Never, Small};
        let auto = |near, whole, fast| super::thumbnails("automatic", near, whole, fast);
        assert_eq!(auto(Some(true), false, None), Always);
        assert_eq!(auto(Some(true), true, Some(true)), Small);
        assert_eq!(auto(None, false, None), Never);
        assert_eq!(auto(Some(false), false, Some(false)), Never);
        assert_eq!(auto(None, false, Some(true)), Small);
        assert_eq!(auto(Some(false), true, Some(true)), Small);
        assert_eq!(super::thumbnails("never", None, false, Some(true)), Never);
        assert_eq!(super::thumbnails("always", None, false, None), Always);
    }

    /// A plan's end asks again how full the mounts it wrote to or freed
    /// room on are, and no other: a path below a mount names it, its
    /// sibling's name beginning the same does not.
    #[test]
    fn a_plan_touches_the_mounts_it_names_a_path_under() {
        use skwir_core::op::Change;
        use std::path::{Path, PathBuf};
        let (one, two, three) = (
            Path::new("/run/user/1000/skwir/remote/one"),
            Path::new("/run/user/1000/skwir/remote/two"),
            Path::new("/run/user/1000/skwir/remote/three"),
        );
        let changes = [
            Change::Move {
                from: PathBuf::from("/home/me/a"),
                to: one.join("a"),
            },
            Change::Create {
                path: PathBuf::from("/run/user/1000/skwir/remote/twofold/b"),
                directory: true,
                words: None,
            },
            Change::Delete {
                path: three.join("c"),
            },
        ];
        let mut touched = super::touched(vec![one, two, three], &changes);
        touched.sort();
        assert_eq!(touched, [one.to_path_buf(), three.to_path_buf()]);
        assert!(super::touched(vec![two], &changes).is_empty());
    }

    #[test]
    fn kde_s_desktop_tries_kio_before_gvfs() {
        let on = |desktops: &[&str]| {
            super::order_on(&desktops.iter().map(|d| (*d).to_owned()).collect::<Vec<_>>())
        };
        // Off Linux, the system's own shares first, whatever the desktop.
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(on(&["KDE"]), "system rclone sshfs fstab gvfs kio kmtp");
            return;
        }
        assert_eq!(on(&["KDE"]), "sshfs kio kmtp gvfs rclone fstab");
        assert_eq!(
            on(&["X-Generic", "kde"]),
            "sshfs kio kmtp gvfs rclone fstab"
        );
        assert_eq!(on(&["GNOME"]), "sshfs kmtp gvfs kio rclone fstab");
        assert_eq!(on(&["Hyprland"]), "sshfs kmtp gvfs kio rclone fstab");
        assert_eq!(on(&[]), "sshfs kmtp gvfs kio rclone fstab");
    }

    use super::*;
    use crate::connector::Connected;

    /// Connects nothing, slowly, counting how many connect at once.
    #[derive(Default)]
    struct Slow {
        now: AtomicUsize,
        most: AtomicUsize,
    }

    impl Connector for Slow {
        fn id(&self) -> &'static str {
            "slow"
        }
        fn title(&self) -> Words {
            Words::as_written("slow")
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Sftp]
        }
        fn unavailable(&self) -> Option<Words> {
            None
        }
        fn connect(
            &self,
            address: &Address,
            _: &Arc<dyn Ask>,
            _: &CancelToken,
        ) -> anyhow::Result<Connected> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(150));
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok(Connected {
                point: PathBuf::from("/nowhere"),
                address: address.top(),
            })
        }
        fn recognises(&self, _: &Remote) -> Option<Recognised> {
            None
        }
        fn disconnect(&self, _: &Remote, _: &Recognised, _: &CancelToken) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct Nobody;

    impl Ask for Nobody {
        fn secret(&self, _: &Words, _: &Words, _: &CancelToken) -> Option<String> {
            None
        }
        fn confirm(&self, _: &Words, _: &Words, _: &CancelToken) -> bool {
            false
        }
    }

    /// Fails as told, asking the window first when `asks`; or connects.
    struct Tool {
        id: &'static str,
        fails: Option<&'static str>,
        asks: bool,
        tried: AtomicUsize,
    }

    impl Tool {
        fn new(id: &'static str, fails: Option<&'static str>, asks: bool) -> Arc<Tool> {
            Arc::new(Tool {
                id,
                fails,
                asks,
                tried: AtomicUsize::new(0),
            })
        }
    }

    impl Connector for Tool {
        fn id(&self) -> &'static str {
            self.id
        }
        fn title(&self) -> Words {
            Words::as_written(self.id)
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Nfs]
        }
        fn unavailable(&self) -> Option<Words> {
            None
        }
        fn connect(
            &self,
            address: &Address,
            ask: &Arc<dyn Ask>,
            cancel: &CancelToken,
        ) -> anyhow::Result<Connected> {
            self.tried.fetch_add(1, Ordering::SeqCst);
            if self.asks {
                ask.confirm(&Words::as_written("Trust it?"), &Words::default(), cancel);
            }
            if let Some(why) = self.fails {
                anyhow::bail!("{why}");
            }
            Ok(Connected {
                point: PathBuf::from("/mounted"),
                address: address.top(),
            })
        }
        fn recognises(&self, _: &Remote) -> Option<Recognised> {
            None
        }
        fn disconnect(&self, _: &Remote, _: &Recognised, _: &CancelToken) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// A tool whose servers are the addresses its file holds, a line each:
    /// a configuration another program changes.
    struct Configured(PathBuf);

    impl Connector for Configured {
        fn id(&self) -> &'static str {
            "configured"
        }
        fn title(&self) -> Words {
            Words::as_written("configured")
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Sftp]
        }
        fn unavailable(&self) -> Option<Words> {
            None
        }
        fn connect(
            &self,
            _: &Address,
            _: &Arc<dyn Ask>,
            _: &CancelToken,
        ) -> anyhow::Result<Connected> {
            anyhow::bail!("connects nothing")
        }
        fn recognises(&self, _: &Remote) -> Option<Recognised> {
            None
        }
        fn disconnect(&self, _: &Remote, _: &Recognised, _: &CancelToken) -> anyhow::Result<()> {
            Ok(())
        }
        fn servers(&self, _: &CancelToken) -> connector::Named {
            let servers = std::fs::read_to_string(&self.0)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| Address::parse(line).ok())
                .map(|address| connector::Server {
                    label: address.label(),
                    address,
                    kind: String::new(),
                    kept: servers::Kept::Tool(servers::Keeper::Fstab),
                })
                .collect();
            connector::Named {
                servers,
                from: vec![connector::Source::File(self.0.clone())],
            }
        }
    }

    /// The servers a tool's configuration names are read again once
    /// another program changes it, its folder made after it was first
    /// read: never only at a start or a refresh asked. None is asked.
    #[test]
    fn servers_another_program_changes_are_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        let file = tmp.path().join("its configuration").join("servers");
        let remotes = Remotes::with(vec![
            Arc::new(Configured(file.clone())) as Arc<dyn Connector>
        ]);
        app.plugins
            .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
        let until = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "{what}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        until("the first read ended", &|| {
            remotes.state.lock().servers_reading.is_none()
        });
        let before = remotes.refreshes();
        let listed = |address: &str| {
            let address = Address::parse(address).unwrap();
            remotes
                .state
                .lock()
                .servers
                .iter()
                .any(|s| s.address == address)
        };
        std::fs::create_dir(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "sftp://me@example.com/\n").unwrap();
        until("the file made was read", &|| {
            listed("sftp://me@example.com/")
        });
        // Written again as tools write: beside it, then in its place.
        let beside = file.with_extension("new");
        std::fs::write(&beside, "sftp://me@example.com/\nsftp://it's@nas.local/\n").unwrap();
        std::fs::rename(&beside, &file).unwrap();
        until("the file replaced was read", &|| {
            listed("sftp://it's@nas.local/")
        });
        std::fs::write(&file, "sftp://it's@nas.local/\n").unwrap();
        until("a server removed is gone", &|| {
            !listed("sftp://me@example.com/")
        });
        assert_eq!(remotes.refreshes(), before, "no refresh was asked");
    }

    /// [`Configured`], counting how often its servers are read.
    struct Counted(Configured, Arc<std::sync::atomic::AtomicUsize>);

    impl Connector for Counted {
        fn id(&self) -> &'static str {
            "counted"
        }
        fn title(&self) -> Words {
            Words::as_written("counted")
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Sftp]
        }
        fn unavailable(&self) -> Option<Words> {
            None
        }
        fn connect(
            &self,
            _: &Address,
            _: &Arc<dyn Ask>,
            _: &CancelToken,
        ) -> anyhow::Result<Connected> {
            anyhow::bail!("connects nothing")
        }
        fn recognises(&self, _: &Remote) -> Option<Recognised> {
            None
        }
        fn disconnect(&self, _: &Remote, _: &Recognised, _: &CancelToken) -> anyhow::Result<()> {
            Ok(())
        }
        fn servers(&self, cancel: &CancelToken) -> connector::Named {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.0.servers(cancel)
        }
    }

    /// A window coming to the front reads what no watch tells once in
    /// `REFRESHED_WITHIN`, one read at a time, and the servers only where
    /// what they were read from could not be watched: a window gone back
    /// and forth asks nothing of the tools whose files are followed.
    #[test]
    fn coming_to_the_front_reads_again_once_and_only_what_is_not_watched() {
        use std::sync::atomic::Ordering;
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        let file = tmp.path().join("servers");
        std::fs::write(&file, "sftp://me@example.com/\n").unwrap();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let remotes =
            Remotes::with(vec![
                Arc::new(Counted(Configured(file.clone()), Arc::clone(&reads)))
                    as Arc<dyn Connector>,
            ]);
        app.plugins
            .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
        let until = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "{what}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let ended = || {
            let state = remotes.state.lock();
            state.servers_reading.is_none() && !state.looking
        };
        until("the first read ended", &ended);
        let first = reads.load(Ordering::SeqCst);
        remotes.looked_at(&app);
        let looked = remotes.state.lock().looked;
        assert!(looked.is_some(), "read as the window came to the front");
        until("the window's read ended", &ended);
        assert_eq!(reads.load(Ordering::SeqCst), first, "its file is followed");
        remotes.looked_at(&app);
        assert_eq!(remotes.state.lock().looked, looked, "once in a while");
        // Where they were read could not be watched: read again.
        {
            let mut state = remotes.state.lock();
            state.looked = None;
            state.sources = None;
        }
        remotes.looked_at(&app);
        until("the window's read ended", &ended);
        assert_eq!(reads.load(Ordering::SeqCst), first + 1);
        // A mount come while no one looked is asked as a window next comes
        // to the front, however soon after the last.
        remotes
            .state
            .lock()
            .ask_owed
            .insert(PathBuf::from("/example/mounted since"));
        remotes.looked_at(&app);
        assert!(remotes.state.lock().ask_owed.is_empty());
    }

    /// A refresh that lists the bridges, dropped as older by a read the
    /// table told that kept them as listed before, is told it listed later
    /// (the mounts read again from its listing): never lost to it. A
    /// slower listing begun before never takes a later one's place.
    #[test]
    fn a_bridge_listed_later_is_never_lost_to_a_read_that_kept_it() {
        let point = PathBuf::from("/run/user/1000/gvfs");
        let share = |name: &str| {
            Arc::new(Remote {
                point: point.join(name),
                fstype: "fuse.gvfsd-fuse".into(),
                source: "gvfsd-fuse".into(),
                device: None,
            })
        };
        let at = |n: u64| HashMap::from([(point.clone(), n)]);
        let mut state = State::default();
        state.keep_listing(
            point.clone(),
            3,
            vec![share("smb-share:server=nas,share=a")],
        );
        state.shown_bridges = at(3);
        // A refresh lists from 5, a share GVfs mounted since among them;
        // a read the table told began at 6, kept the listing of 3, and
        // stored as the newest.
        state.keep_listing(
            point.clone(),
            5,
            vec![
                share("smb-share:server=nas,share=a"),
                share("smb-share:server=nas,share=b's"),
            ],
        );
        state.shown_bridges = at(3);
        assert!(state.listed_later(&at(5)), "the refresh's listing is read");
        assert!(!state.listed_later(&at(3)), "what is shown is no later");
        state.keep_listing(point.clone(), 4, Vec::new());
        assert_eq!(state.bridged[&point].0, 5, "a slower listing begun before");
        assert_eq!(state.bridged[&point].1.len(), 2);
        state.shown_bridges = at(5);
        assert!(!state.listed_later(&at(5)), "shown: nothing left to read");
        state.bridged.clear();
        assert!(!state.listed_later(&at(9)), "a bridge let go of");
    }

    /// What an address names below a mount is the mount's: never a folder
    /// of this machine above it.
    #[test]
    fn a_path_below_a_mount_stays_in_it() {
        let point = Path::new("/run/user/1/skwir/remote/sftp-h");
        assert_eq!(super::join(point, b""), point);
        assert_eq!(
            super::join(point, b"it's/a b/./c"),
            point.join("it's").join("a b").join("c")
        );
        assert_eq!(super::join(point, b"../../../../home"), point.join("home"));
        assert_eq!(super::join(point, b"a/../../b"), point.join("b"));
    }

    /// The connectors installed are tried in their order: one that fails
    /// without asking anything leaves the address to the next; one the
    /// user answered speaks for the attempt.
    #[test]
    fn a_tool_that_fails_unasked_leaves_the_address_to_the_next() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let ask: Arc<dyn Ask> = Arc::new(Nobody);
        let address = Address::parse("nfs://nas/srv/it's photos").unwrap();
        let reach = |tools: &[&Arc<Tool>]| {
            let remotes = Remotes::with(
                tools
                    .iter()
                    .map(|t| Arc::clone(t) as Arc<dyn Connector>)
                    .collect(),
            );
            remotes.reach(&app, &address, &ask, &CancelToken::new())
        };

        let (unasked, next) = (
            Tool::new("first", Some("Mount point does not exist"), false),
            Tool::new("second", None, false),
        );
        assert_eq!(reach(&[&unasked, &next]).unwrap(), Path::new("/mounted"));
        assert_eq!(next.tried.load(Ordering::SeqCst), 1);

        let (answered, next) = (
            Tool::new("first", Some("refused"), true),
            Tool::new("second", None, false),
        );
        let error = reach(&[&answered, &next]).unwrap_err();
        assert_eq!(format!("{error:#}"), "refused");
        assert_eq!(next.tried.load(Ordering::SeqCst), 0, "never asked again");

        let (one, other) = (
            Tool::new("first", Some("no export"), false),
            Tool::new("second", Some("no entry"), false),
        );
        let error = reach(&[&one, &other]).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "first: no export and second: no entry",
            "a list, joined as the reader's language joins one"
        );
    }

    /// Two panes opening one server at once connect one after the other,
    /// never together: the second would ask the password again and mount
    /// over the first.
    #[test]
    fn connections_to_one_address_take_turns() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let slow = Arc::new(Slow::default());
        let remotes = Remotes::with(vec![Arc::clone(&slow) as Arc<dyn Connector>]);
        let ask: Arc<dyn Ask> = Arc::new(Nobody);
        std::thread::scope(|scope| {
            for path in ["/a", "/b"] {
                let (remotes, app, ask) = (&remotes, &app, &ask);
                scope.spawn(move || {
                    let address = Address::parse(&format!("sftp://me@example.com{path}")).unwrap();
                    remotes.reach(app, &address, ask, &CancelToken::new())
                });
            }
        });
        assert_eq!(slow.most.load(Ordering::SeqCst), 1);
    }

    /// What other programs left to be sent is said on the mount's place
    /// as it goes down, and nothing else is read for it: no refresh,
    /// whole or of the mounts (each read the servers, starting rclone,
    /// and asked every server how full it is). Against a stand-in of the
    /// mount's remote control, counted.
    #[cfg(unix)]
    #[test]
    fn what_waits_to_be_sent_is_said_without_reading_anything_again() {
        use std::io::{BufRead, BufReader, Read, Write};
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        let remotes = Remotes::with(Vec::new());
        app.plugins
            .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
        let point = tmp.path().join("example uploads' mount");
        let socket = crate::rclone_copy::socket_for(&point).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // The queue as rclone says it, an answer a question: three files,
        // two, two again, then none.
        let (served, asked) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            for count in [3, 2, 2, 0] {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let queue: Vec<_> = (0..count)
                    .map(|id| serde_json::json!({ "id": id, "name": format!("file {id}") }))
                    .collect();
                let text = serde_json::json!({ "queue": queue }).to_string();
                let mut stream = reader.into_inner();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
                served.send(count).unwrap();
            }
        });
        let before = remotes.refreshes();
        remotes.look_at_uploads(&app, &point);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut said = Vec::new();
        while said.last() != Some(&0) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            said.push(asked.recv_timeout(left).expect("asked until none waits"));
        }
        while remotes.state.lock().watching.contains(&point) {
            assert!(std::time::Instant::now() < deadline, "still watched");
            std::thread::yield_now();
        }
        assert_eq!(said, [3, 2, 2, 0]);
        assert!(remotes.state.lock().waiting.is_empty());
        assert_eq!(remotes.refreshes(), before, "nothing read again");
        let _ = std::fs::remove_file(&socket);
    }

    /// A tool that mounts a server's folders one by one (as kio-fuse):
    /// only what `as_mounted` asks of it.
    struct EachPath;

    impl Connector for EachPath {
        fn id(&self) -> &'static str {
            "each-path"
        }
        fn title(&self) -> Words {
            Words::as_written("each path")
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Sftp]
        }
        fn unavailable(&self) -> Option<Words> {
            None
        }
        fn connect(
            &self,
            _: &Address,
            _: &Arc<dyn Ask>,
            _: &CancelToken,
        ) -> anyhow::Result<Connected> {
            anyhow::bail!("not asked")
        }
        fn recognises(&self, _: &Remote) -> Option<Recognised> {
            None
        }
        fn disconnect(&self, _: &Remote, _: &Recognised, _: &CancelToken) -> anyhow::Result<()> {
            Ok(())
        }
        fn mounts_each_path(&self) -> bool {
            true
        }
    }

    fn kept(label: &str, address: &str) -> connector::Server {
        connector::Server {
            address: Address::parse(address).unwrap(),
            label: Words::as_written(label.to_owned()),
            kind: String::new(),
            kept: servers::Kept::Skwir,
        }
    }

    fn mount(address: &str, point: &str, connector: Option<Arc<dyn Connector>>) -> super::Mounted {
        super::Mounted {
            remote: Arc::new(Remote {
                point: PathBuf::from(point),
                fstype: "fuse.sshfs".into(),
                source: "nas:/".into(),
                device: None,
            }),
            connector,
            recognised: Recognised {
                address: Some(Address::parse(address).unwrap()),
                label: Words::as_written("me@nas"),
                ours: true,
                device: None,
            },
            ejects: true,
        }
    }

    /// The mount's own place, as `publish` makes it before a server takes
    /// it: at its root, under its own name and key, with its eject.
    fn mount_place(m: &super::Mounted) -> Place {
        let mut place = Place::new(
            super::GROUP,
            m.recognised.label.clone(),
            "folder-remote",
            Location::from_path(&m.remote.point),
        );
        place.key = Some("remote:sftp://me@nas/".into());
        place.eject = Some(Invocation::new("remote.disconnect"));
        place
    }

    /// Each server a mount holds, found in it by its own folder; one above
    /// the mount, or on another server, in none. An SMB server's folder is
    /// below its share's mount.
    #[test]
    fn a_server_is_found_in_the_mount_that_holds_its_folder() {
        let listed = [
            kept("Photos", "sftp://nas/srv/photos"),
            kept("Music", "sftp://nas/srv/music"),
            kept("Above", "sftp://nas2/srv"),
            kept("Elsewhere", "sftp://other/"),
            kept("Year", "smb://nas/photos/2024"),
        ];
        let mounted = [
            mount("sftp://me@nas/", "/run/m/nas", None),
            mount("sftp://nas2/srv/photos", "/run/m/nas2", None),
            mount("smb://nas/photos/", "/run/m/share", None),
        ];
        let held: Vec<Option<(usize, &[u8])>> = super::held(&listed, &mounted);
        assert_eq!(
            held,
            [
                Some((0, b"srv/photos".as_slice())),
                Some((0, b"srv/music".as_slice())),
                None,
                None,
                Some((2, b"2024".as_slice())),
            ]
        );
    }

    /// A server kept with a folder, mounted, is still its own place: its
    /// name, its key (what the user renamed, moved or hid it as), its Edit
    /// and Remove, opening its folder in the mount, not the mount's root;
    /// with the mount's eject. Two of one server are two places.
    #[test]
    fn a_server_mounted_opens_its_own_folder_as_its_own_place() {
        let m = mount("sftp://me@nas/", "/run/m/nas", None);
        // The server's path below the mount's point, name by name, as this
        // system joins a path.
        let point = Path::new("/run/m/nas");
        for (server, folder) in [
            (
                kept("Photos", "sftp://nas/srv/photos"),
                point.join("srv").join("photos"),
            ),
            (
                kept("Music", "sftp://nas/srv/music"),
                point.join("srv").join("music"),
            ),
        ] {
            let below = m
                .recognised
                .address
                .as_ref()
                .unwrap()
                .below(&server.address)
                .unwrap();
            let place = super::as_mounted(mount_place(&m), &server, &m, below, None, &[]);
            assert_eq!(place.title, server.label);
            assert_eq!(place.key, Some(format!("remote:{}", server.address)));
            assert_eq!(place.location, Location::from_path(folder));
            assert_eq!(place.eject, Some(Invocation::new("remote.disconnect")));
            assert!(place.activate.is_none());
            let edit = place
                .menu
                .iter()
                .find(|item| &*item.command.name == "remote.edit_server")
                .expect("its Edit");
            assert_eq!(edit.command.args["id"], server.address.to_string());
        }
        // One at the server's top opens the mount's root.
        let top = kept("NAS", "sftp://nas/");
        let place = super::as_mounted(mount_place(&m), &top, &m, b"", None, &[]);
        assert_eq!(place.location, Location::from_path("/run/m/nas"));
    }

    /// A server mounted earlier offers, from its place, to connect again
    /// (ADR-149): the mount's own address, which `remote.connect` opens
    /// below at the folder a pane could not show, there again once
    /// connected. A connection lost says so.
    #[test]
    fn a_server_disconnected_offers_to_connect_again_where_it_was() {
        let address = Address::parse("sftp://me@nas/srv").unwrap();
        let label = Words::as_written("me@nas");
        let reopen = super::reopen(&label, &address, Path::new("/run/m/nas"), false);
        assert_eq!(reopen.at, Path::new("/run/m/nas"));
        assert_eq!(&*reopen.command.name, "remote.connect");
        assert_eq!(reopen.command.args["address"], "sftp://me@nas/srv");
        assert!(reopen.title.english().ends_with(" is not connected"));
        assert!(reopen.label.english().starts_with("Connect to "));
        // As `remote.connect` opens it, given the folder below.
        let below = skwir_core::registry::Reopen::below(Path::new("photos/it's été")).unwrap();
        assert_eq!(
            skwir_core::remotes::address_below(&address.location(), below).to_string(),
            "sftp://me@nas/srv/photos/it's été"
        );
        let lost = super::reopen(&label, &address, Path::new("/run/m/nas"), true);
        assert!(lost.title.english().starts_with("The connection to "));
    }

    /// While Skwir connects to a server again, what a pane offers for it
    /// says so in place of its button, which is not offered again
    /// meanwhile (ADR-149, 4); once the connection ends, the button is back.
    #[test]
    fn a_server_being_connected_to_says_so_where_it_is_offered() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let remotes = Remotes::with(Vec::new());
        app.plugins
            .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
        let address = Address::parse("sftp://me@nas.example/srv").unwrap();
        let at = tmp.path().join("it's nas, once mounted");
        remotes.state.lock().mounted_at.push((
            address.clone(),
            at.clone(),
            Words::as_written("me@nas"),
        ));
        let busy = || {
            app.registry
                .reopens
                .find(|reopen| reopen.at == at)
                .map(|reopen| reopen.busy.as_ref().map(Words::english))
        };
        remotes.publish(&app);
        assert_eq!(busy(), Some(None), "offered, its button shown");
        // Another folder of the same server, being connected to.
        let below = Address::parse("sftp://me@nas.example/srv/it's photos").unwrap();
        let connecting = Connecting::begin(&remotes, &app, &below);
        assert_eq!(busy(), Some(Some("Connecting…".to_owned())));
        drop(connecting);
        assert_eq!(busy(), Some(None), "the button back");
    }

    /// A server whose mount was lost connects again in its own folder; one
    /// kio-fuse holds is opened below its mount by its address, which asks
    /// kio-fuse for that folder.
    #[test]
    fn a_server_lost_or_held_by_kio_fuse_opens_by_its_address() {
        let server = kept("Photos", "sftp://nas/srv/photos");
        let m = mount("sftp://me@nas/", "/run/m/nas", None);
        let mut lost = mount_place(&m);
        lost.activate = Some(Invocation::navigate(
            &Address::parse("sftp://me@nas/").unwrap().location(),
        ));
        let place = super::as_mounted(lost, &server, &m, b"srv/photos", None, &[]);
        assert_eq!(
            place.activate,
            Some(Invocation::navigate(&server.address.location()))
        );

        let kio = mount("sftp://me@nas/", "/run/m/kio", Some(Arc::new(EachPath)));
        let place = super::as_mounted(mount_place(&kio), &server, &kio, b"srv/photos", None, &[]);
        assert_eq!(place.location, server.address.location());
        let top = kept("NAS", "sftp://nas/");
        let place = super::as_mounted(mount_place(&kio), &top, &kio, b"", None, &[]);
        assert_eq!(place.location, Location::from_path("/run/m/kio"));
        // Once kio-fuse answered where the folder is, the place stands for
        // it there: the trail and the tab name it by the place. An answer
        // below a mount that went is asked again.
        let there = Path::new("/run/m/kio/srv/photos");
        let place = super::as_mounted(
            mount_place(&kio),
            &server,
            &kio,
            b"srv/photos",
            Some(there),
            &[],
        );
        assert_eq!(place.location, Location::from_path(there));
        let gone = Path::new("/run/m/old/srv/photos");
        let place = super::as_mounted(
            mount_place(&kio),
            &server,
            &kio,
            b"srv/photos",
            Some(gone),
            &[],
        );
        assert_eq!(place.location, server.address.location());
    }

    /// Only a server kept with a folder below a mount's top takes a place
    /// of its own; a mount gives way to such servers alone, and never a
    /// device's, whose own place stays (the server beside it, among the
    /// servers). A server at a mount's top is drawn as the mount's place,
    /// as before (what the user arranged the mount's place as holds).
    #[test]
    fn a_mount_gives_way_only_to_servers_kept_with_a_folder() {
        let listed = [
            kept("Photos", "sftp://nas/srv/photos"),
            kept("DCIM", "mtp://Example_Phone/Internal storage/DCIM"),
            kept("Export", "nfs://nas2/export"),
            kept("Music", "sftp://nas3/srv/music"),
            kept("NAS3", "sftp://nas3/"),
        ];
        let mut phone = mount("mtp://Example_Phone/", "/run/m/phone", None);
        phone.recognised.device = Some(crate::connector::Device::Phone);
        let mounted = [
            mount("sftp://me@nas/", "/run/m/nas", None),
            phone,
            mount("nfs://nas2/export", "/run/m/export", None),
            mount("sftp://nas3/", "/run/m/nas3", None),
        ];
        let held = super::held(&listed, &mounted);
        let super::Holds { in_folder, at_top } = super::holds(&held, &mounted);
        assert_eq!(in_folder, [true, true, false, true]);
        assert_eq!(at_top, [None, None, Some(2), Some(4)]);
        // The phone's place stays the phone's; its server is drawn beside
        // it, among the servers, under its own name and key.
        let below = mounted[1]
            .recognised
            .address
            .as_ref()
            .unwrap()
            .below(&listed[1].address)
            .unwrap();
        let mut device_place = mount_place(&mounted[1]);
        device_place.group = super::DEVICES.to_owned();
        let place = super::as_mounted(device_place, &listed[1], &mounted[1], below, None, &[]);
        assert_eq!(place.group, super::GROUP);
        assert_eq!(place.key, Some(format!("remote:{}", listed[1].address)));
        assert_eq!(
            place.location,
            Location::from_path(
                Path::new("/run/m/phone")
                    .join("Internal storage")
                    .join("DCIM")
            )
        );
    }
}
