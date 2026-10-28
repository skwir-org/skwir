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

pub mod address;
#[cfg(target_os = "linux")]
mod bus;
mod configure;
pub mod connector;
pub mod fstab;
pub mod gio_copy;
pub mod gvfs;
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
mod ssh_mount;
pub mod sshfs;
pub mod system_shares;
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

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connector, Offered, Password, Recognised, Server};

pub const ID: &str = "remote";

/// The sidebar's section of remotes.
pub const GROUP: &str = "network";

/// The sidebar's section of this machine's devices, where a phone or a
/// camera GVfs reads is.
const DEVICES: &str = "devices";

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
}

#[derive(Default)]
struct State {
    /// Cancelled when the plugin is disabled.
    lifetime: Option<CancelToken>,
    /// The servers the tools' configurations name, as last read.
    servers: Vec<Server>,
    /// Whether each host is on the local network, as found out.
    local: HashMap<String, Option<bool>>,
    /// Mounts being connected or let go of, by point (or address).
    busy: HashMap<String, &'static str>,
    /// Points whose program is gone (`ENOTCONN`): drawn unavailable.
    lost: std::collections::HashSet<PathBuf>,
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
    /// Bumped by each refresh, so a slower one never wins.
    generation: u64,
    /// The mounts as last read (`Remotes::mounted`): what every publish
    /// shows, whoever asks for it.
    mounted: Arc<Vec<Mounted>>,
    /// The remotes connected at login, by their entry's id (`login::id_of`).
    at_login: std::collections::HashSet<String>,
    /// Mounts whose program is being asked whether it still answers: one
    /// question at a time each, never another queued behind one a server
    /// gone holds for good.
    asking: std::collections::HashSet<PathBuf>,
    /// The devices plugged in each connector offers, mounted or not
    /// (ADR-096), as it last told.
    devices: HashMap<&'static str, Vec<Offered>>,
}

impl State {
    /// Whether each mount's server is near, where its name says so (an
    /// address, `nas.local`); a name to resolve is found out after. A
    /// device is no server: its name is never looked up.
    fn note_said(&mut self) {
        let mounted = Arc::clone(&self.mounted);
        for m in mounted.iter() {
            if let Some(address) = &m.recognised.address
                && address.scheme.device().is_none()
                && !self.local.contains_key(&address.host)
                && let Some(near) = lan::said(&address.host)
            {
                self.local.insert(address.host.clone(), Some(near));
            }
        }
    }
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
        let options = options::of(app, &address.top());
        let connected = connector
            .connect_with(address, &options, &asked, cancel)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
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
    pub fn label_of(&self, address: &Address) -> String {
        self.offered_at(address)
            .map_or_else(|| address.label(), |offered| offered.name)
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
        self.refresh(app);
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
        let offered: Vec<Offered> = self
            .state
            .lock()
            .devices
            .values()
            .flatten()
            .cloned()
            .collect();
        reach::all()
            .iter()
            .flat_map(reach::bridged)
            .filter_map(|remote| {
                let known = self.connectors.iter().find_map(|c| {
                    let mut recognised = c.recognises(&remote)?;
                    // A device by the name its volume has, mounted or not.
                    if let Some(offered) = recognised.address.as_ref().and_then(|a| {
                        offered
                            .iter()
                            .find(|o| a.scheme.device().is_some() && o.address.same_server(a))
                    }) {
                        recognised.label.clone_from(&offered.name);
                    }
                    Some((Arc::clone(c), recognised))
                });
                Some(match known {
                    Some((connector, recognised)) => Mounted {
                        remote,
                        connector: Some(connector),
                        recognised,
                    },
                    // What kio-fuse holds of no remote (an archive KIO
                    // reads, a local folder) is no mount to show.
                    None if remote.fstype == kio::BRIDGE => return None,
                    None => Mounted {
                        recognised: Recognised {
                            label: remote.point.file_name().map_or_else(
                                || remote.source.to_string_lossy().into_owned(),
                                |n| n.to_string_lossy().into_owned(),
                            ),
                            address: None,
                            ours: false,
                            device: None,
                        },
                        remote,
                        connector: None,
                    },
                })
            })
            .collect()
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
                Some(held) if !cancel.is_cancelled() => anyhow::anyhow!("{held}"),
                _ => error,
            })
    }

    /// Why the device at `address` opens nowhere, where another program of
    /// the user's holds it open (KDE's `kiod6`, a camera's program): one
    /// program at a time uses a device over USB. Reads `/proc`: on a
    /// worker, once opening it failed.
    fn held_by(&self, address: &Address) -> Option<String> {
        let device = address.scheme.device()?;
        let node = self.offered_at(address)?.node?;
        // GVfs's own program for it, still letting go of the open that
        // failed, is not another's (one for a camera holding a phone is).
        let asked = gvfs::backend_of(address.scheme);
        let mut programs = holders::of(&node);
        programs.retain(|program| *program != asked);
        let (them, have) = match programs.as_slice() {
            [] => return None,
            [one] => (one.clone(), "has"),
            [first @ .., last] => (format!("{} and {last}", first.join(", ")), "have"),
        };
        let quit = if programs.len() == 1 { &them } else { "them" };
        let kind = match device {
            connector::Device::Phone => "a phone",
            connector::Device::Camera => "a camera",
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
            Some((holder, why)) => format!(
                "{them} {have} it open, and only one program at a time can use {kind}. Skwir would open it through {holder}, but {why}; else quit {quit}, then try again."
            ),
            None => format!(
                "{them} {have} it open, and only one program at a time can use {kind}. Quit {quit}, then try again."
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
            anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        };
        for mounted in self.mounted() {
            if let Some(below) = mounted
                .recognised
                .address
                .as_ref()
                .and_then(|top| top.below(address))
            {
                // A mount whose program died answers nothing: mounted again.
                // A kio-fuse gone is let go of whole: the bus starts another,
                // which mounts in a folder of its own.
                match std::fs::symlink_metadata(&mounted.remote.point) {
                    Err(error) if lost(&error) => {
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
                    _ => {}
                }
                match &mounted.connector {
                    // A server kio-fuse holds, but maybe not this folder of
                    // it yet: asked for it.
                    Some(connector) if connector.mounts_each_path() => {
                        let connected = self.connect_keyed(app, connector, address, ask, cancel)?;
                        let below = connected.address.below(address).unwrap_or_default();
                        return Ok(join(&connected.point, below));
                    }
                    _ => return Ok(join(&mounted.remote.point, below)),
                }
            }
        }
        let mut why_not = Vec::new();
        let mut failed: Vec<(&'static str, anyhow::Error)> = Vec::new();
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
        if !failed.is_empty() {
            let said: Vec<String> = failed
                .iter()
                .map(|(tool, error)| format!("{tool}: {error:#}"))
                .collect();
            anyhow::bail!("{}", said.join("; "));
        }
        if why_not.is_empty() {
            anyhow::bail!(
                "no tool Skwir knows opens {} addresses",
                address.scheme.title()
            );
        }
        anyhow::bail!("nothing installed can open it: {}", why_not.join("; "))
    }

    /// Read again what is mounted and publish it, then what servers are
    /// known: on a worker. What is mounted is read from memory and
    /// published at once; the servers the tools know (`rclone
    /// listremotes`, ssh's configuration) and whether each mount's server
    /// is near take longer, and are published after, unless a later
    /// refresh began meanwhile: the mounts never wait on them.
    fn refresh(&self, app: &App) {
        let Some(lifetime) = self.lifetime() else {
            return;
        };
        let generation = {
            let mut state = self.state.lock();
            state.generation += 1;
            state.generation
        };
        let this = self.clone();
        let weak = app.weak();
        app.scheduler.spawn(
            Priority::Background,
            "remote-refresh",
            lifetime.clone(),
            move || {
                let mounted = Arc::new(this.mounted());
                let current = |state: &State| state.generation == generation;
                {
                    let mut state = this.state.lock();
                    if !current(&state) {
                        return;
                    }
                    state.mounted = Arc::clone(&mounted);
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
                let Some(app) = weak.upgrade() else {
                    return;
                };
                this.publish(&app);
                this.ask_alive(&app, &mounted, &lifetime);
                // The servers kept (Skwir's list, KDE's network folders),
                // then the tools' that are not those.
                let mut servers = servers::listed(&app);
                drop(app);
                for connector in this.connectors.iter() {
                    if connector.unavailable().is_none() {
                        for server in connector.servers(&lifetime) {
                            if !servers.iter().any(|s| s.address == server.address) {
                                servers.push(server);
                            }
                        }
                    }
                }
                // Whether each server is near, for its thumbnails. A
                // device is near, its name (a serial) never looked up.
                let hosts: Vec<String> = mounted
                    .iter()
                    .filter_map(|m| m.recognised.address.as_ref())
                    .filter(|a| a.scheme.device().is_none())
                    .map(|a| a.host.clone())
                    .collect();
                let mut local = this.state.lock().local.clone();
                for host in hosts {
                    local
                        .entry(host.clone())
                        .or_insert_with(|| lan::local(&host));
                }
                // Which of them connect at login: their manager's files.
                let at_login = this.login_manager().map_or_else(
                    |_| std::collections::HashSet::new(),
                    |manager| {
                        manager.installed_of(
                            mounted
                                .iter()
                                .filter_map(|m| m.recognised.address.clone())
                                .chain(servers.iter().map(|s| s.address.clone()))
                                .filter_map(|a| configure::login_id(&a)),
                        )
                    },
                );
                {
                    let mut state = this.state.lock();
                    if !current(&state) {
                        return;
                    }
                    if state.servers != servers || state.at_login != at_login {
                        this.records.changed();
                    }
                    state.servers = servers;
                    state.local = local;
                    state.at_login = at_login;
                }
                if let Some(app) = weak.upgrade() {
                    this.publish(&app);
                }
            },
        );
    }

    /// Ask each of `mounted` whether its program still answers, and how
    /// full it is, in the remote's own lane: a server that stopped
    /// answering holds that alone (ADR-077). A mount still being asked is
    /// not asked again.
    fn ask_alive(&self, app: &App, mounted: &Arc<Vec<Mounted>>, lifetime: &CancelToken) {
        /// The mount being asked, no longer once its question ran, or was
        /// dropped unrun.
        struct Asking(Arc<Mutex<State>>, PathBuf);
        impl Drop for Asking {
            fn drop(&mut self) {
                self.0.lock().asking.remove(&self.1);
            }
        }
        let weak = app.weak();
        for m in mounted.iter() {
            let point = m.remote.point.clone();
            if !self.state.lock().asking.insert(point.clone()) {
                continue;
            }
            let asking = Asking(Arc::clone(&self.state), point.clone());
            let (this, weak) = (self.clone(), weak.clone());
            // What an rclone mount of Skwir's can do, asked once a mount.
            let ask_features = m.recognised.ours
                && m.connector.as_ref().is_some_and(|c| c.id() == "rclone")
                && !self.state.lock().features.contains_key(&point);
            let lifetime_of_ask = lifetime.clone();
            app.scheduler.spawn_in(
                m.remote.lane(),
                Priority::Background,
                "remote-alive",
                lifetime.clone(),
                move || {
                    let _asking = asking;
                    let gone = std::fs::symlink_metadata(&point).is_err_and(|e| lost(&e));
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

    /// Read what is mounted and publish it at once, on this thread: a
    /// connection was just made, its folder is shown next, and F5 there
    /// must find what its program is told (`Known::forget`). A refresh
    /// that began before stores nothing older.
    fn publish_mounted(&self, app: &App) {
        let mounted = Arc::new(self.mounted());
        {
            let mut state = self.state.lock();
            state.generation += 1;
            state.mounted = mounted;
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
        for m in mounted.iter() {
            let point = &m.remote.point;
            let address = m.recognised.address.as_ref();
            let key = address.map_or_else(|| point.display().to_string(), ToString::to_string);
            let rule = chosen.get(&key).unwrap_or(&default);
            let near = if m.recognised.device.is_some() {
                Some(true)
            } else {
                address.and_then(|a| state.local.get(&a.host).copied().flatten())
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
            });
            let mut place = match m.recognised.device {
                Some(device) => Place::new(
                    DEVICES,
                    &m.recognised.label,
                    device.icon(),
                    Location::from_path(point),
                ),
                None => Place::new(
                    GROUP,
                    &m.recognised.label,
                    "folder-remote",
                    Location::from_path(point),
                ),
            };
            place.key = Some(format!("remote:{key}"));
            if m.connector
                .as_ref()
                .is_some_and(|c| can_disconnect(c.as_ref(), &m.remote, &m.recognised))
            {
                place.eject = Some(
                    Invocation::new("remote.disconnect")
                        .with("path", PathArg::from(point.as_path())),
                );
            }
            if state.lost.contains(point) {
                place.unavailable =
                    Some("The connection is lost: open it to connect again".to_owned());
                if let Some(address) = address {
                    place.activate = Some(Invocation::navigate(&address.location()));
                }
            }
            place.busy = state
                .busy
                .get(&point.display().to_string())
                .map(|s| (*s).to_owned());
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
            } else if let Some(kept) = address.and_then(|a| {
                state
                    .servers
                    .iter()
                    .find(|s| s.kept.by_address() && a.below(&s.address).is_some())
            }) {
                // A server kept, mounted: its own Edit and Remove.
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
                &offered.name,
                offered.device.icon(),
                offered.address.location(),
            );
            place.key = Some(format!("remote:{}", offered.address));
            place.unavailable = Some("Not mounted. Click to mount and open it.".to_owned());
            places.push(place);
        }
        for server in &state.servers {
            let shown = mounted.iter().any(|m| {
                m.recognised
                    .address
                    .as_ref()
                    .is_some_and(|a| a.below(&server.address).is_some())
            });
            if shown {
                continue;
            }
            let location = server.address.location();
            let mut place = Place::new(GROUP, &server.label, "network-server", location);
            place.key = Some(format!("remote:{}", server.address));
            place.busy = state
                .busy
                .get(&server.address.to_string())
                .map(|s| (*s).to_owned());
            place.menu.extend(forget_item(&server.address, &remembered));
            place.menu.extend(options_item(&server.address));
            place
                .menu
                .extend(configure::server_items(&server.address, &state.at_login));
            if let Some(name) = configure::remote_name(&server.address) {
                let half_made = unfinished.iter().any(|u| u == name);
                place.menu.extend(configure::menu_items(
                    name,
                    configure::signs_in(&server.kind),
                    half_made,
                ));
                // Drawn as not ready, saying why, opened all the same.
                if half_made {
                    place.unavailable = Some(format!(
                        "{name} is {}: Finish Setting Up… in its menu",
                        configure::NOT_FINISHED
                    ));
                }
            } else if server.kept.by_address() {
                place.menu.extend(configure::kept_items(&server.address));
            }
            places.push(place);
        }
        // The servers announced on the local network, found while it is
        // shown (ADR-084): where Avahi and wsdd run.
        if cfg!(target_os = "linux") {
            let mut place = Place::new(GROUP, "Nearby", "network-workgroup", nearby::location());
            place.key = Some("remote:nearby".into());
            places.push(place);
        }
        state.proved_seen = proved;
        drop(state);
        app.registry.places.replace_owner(ID, places);
        app.registry.remotes.replace_owner(ID, known);
        app.wake();
    }
}

/// Whether the user may let go of a mount: Skwir's, GVfs's, one fstab lets
/// them mount; never kio-fuse's, which lets go of no server.
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
    [
        ("automatic", "Automatic"),
        ("never", "Never"),
        ("small", "Small Files"),
        ("always", "Always"),
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
            "Thumbnails",
            skwir_core::registry::ChoiceLook::Icons,
            value == rule,
        );
        item.submenu = Some("Thumbnails".to_owned());
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
            "Forget Password",
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
            "Mount Options…",
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
                format!("The password was not remembered: {why}"),
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

impl Plugin for RemotePlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Remote folders"
    }

    fn description(&self) -> &'static str {
        "Open SFTP, SMB, WebDAV, FTP, NFS and rclone addresses as folders, through sshfs, GVfs, rclone or the system's mounts; phones and cameras through GVfs"
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
            Requirement::program("sshfs", "Opening SFTP addresses through your ssh"),
            Requirement::program(
                "gio",
                "Opening addresses through GVfs, the desktop's remotes",
            ),
            Requirement::program("rclone", "Opening rclone's remotes and other addresses"),
            kmtp::WORKER.requirement("Opening a phone KDE's kmtpd holds"),
        ]
    }

    fn on_disable(&self, _app: &App) {
        let mut state = self.remotes.state.lock();
        if let Some(lifetime) = state.lifetime.take() {
            lifetime.cancel();
        }
        state.devices.clear();
    }

    fn register(&self, app: &App) {
        for transfer in self.remotes.connectors.iter().flat_map(|c| c.transfers()) {
            app.transfers.register(ID, transfer);
        }
        // A copy down that proved a server fast, or slow again, changes
        // its thumbnails' Automatic: published again at its plan's end.
        let (remotes, weak) = (self.remotes.clone(), app.weak());
        app.executor.observe(
            ID,
            Arc::new(move |_| {
                let seen = remotes.state.lock().proved_seen;
                if skwir_core::fs::reach::proved_changed() != seen
                    && let Some(app) = weak.upgrade()
                {
                    remotes.publish(&app);
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
        app.settings.declare(
            SettingSpec::text(
                CONNECTORS,
                "Tools that open addresses",
                "The tools that mount an sftp://, smb:// or other address, tried in this order among those installed: sshfs, kmtp (a phone KDE holds), gvfs, kio, rclone, fstab. On KDE's desktop, kio comes before gvfs.",
                default_order(),
            )
            .page(skwir_core::settings::page::NETWORK)
            .section("Opening addresses"),
        );
        app.settings.declare(
            SettingSpec::choice(
                THUMBNAILS,
                "Thumbnails in remote folders",
                "For a remote you chose nothing for in its menu: Automatic shows them on the local network only, as reading every picture across the internet would be slow.",
                "automatic",
                &[
                    ("automatic", "Automatic"),
                    ("never", "Never"),
                    ("small", "Small files"),
                    ("always", "Always"),
                ],
            )
            .page(skwir_core::settings::page::VIEWS)
            .section("Thumbnails"),
        );
        app.settings.declare(SettingSpec::data(
            CHOSEN,
            "The thumbnails chosen for each remote, by its address",
            toml::Value::Table(toml::map::Map::new()),
        ));
        app.settings.declare(SettingSpec::data(
            options::SETTING,
            "The mount options chosen for each server, by its address",
            toml::Value::Table(toml::map::Map::new()),
        ));
        app.settings.declare(SettingSpec::data(
            rclone_forms::UNFINISHED,
            "The remotes of rclone's Skwir began to set up and did not finish, by name",
            toml::Value::Array(Vec::new()),
        ));
        app.settings.declare(SettingSpec::data(
            REMEMBERED,
            "The servers whose password is kept in the keyring, by their address",
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
            "Connect to server",
            "Open a remote folder by its address (sftp://, smb://, dav://, ftp://, nfs://, afp://, rclone://)",
            move |args, app| {
                let address = Address::parse(args.address.trim())?;
                let _ = &remotes;
                let pane = app.pane_or_active(args.pane);
                if args.tab {
                    app.new_tab(Some(pane), Some(address.location()), false)?;
                } else {
                    app.navigate(pane, &address.location())?;
                }
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<DisconnectArgs, _>(
            ID,
            Topic::Places,
            "remote.disconnect",
            "Disconnect",
            "Let go of a remote folder: what shows it goes home",
            move |args, app| {
                remotes.disconnect(app, &args.path.into_inner(), args.force)?;
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<ThumbnailsArgs, _>(
            ID, Topic::Places,
            "remote.thumbnails",
            "Thumbnails of a remote",
            "Choose when a remote folder's files show thumbnails: automatic, never, small or always",
            move |args, app| {
                anyhow::ensure!(
                    ["automatic", "never", "small", "always"].contains(&args.rule.as_str()),
                    "a rule is automatic, never, small or always"
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
                            format!("The remote's thumbnails were not saved: {error:#}"),
                        );
                    }
                    remotes.refresh(&app);
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
            "Forget password",
            "Remove from the keyring the password Skwir keeps for a server (never one another program kept)",
            move |args, app| {
                let remembered: Vec<String> = app.settings.get(REMEMBERED).unwrap_or_default();
                anyhow::ensure!(
                    remembered.contains(&args.server),
                    "Skwir keeps no password for {}",
                    args.server
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
                                app.notify(NotifyLevel::Info, "The password is forgotten");
                            }
                            Err(error) => app.notify(
                                NotifyLevel::Error,
                                format!("The password was not forgotten: {error:#}"),
                            ),
                        }
                        remotes.refresh(&app);
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
                "Refresh remote folders",
                "Read again what is mounted and which servers the tools know",
                move |_, app| {
                    reach::changed();
                    remotes.refresh(app);
                    Ok(Outcome::Done)
                },
            )
            .shows();
        self.remotes.refresh(app);
    }

    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys
            .iter()
            .any(|k| k == THUMBNAILS || k == CHOSEN || k == REMEMBERED)
        {
            self.remotes.refresh(app);
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
                    app.notify(
                        NotifyLevel::Error,
                        format!("“{label}” was not disconnected: {error:#}"),
                    );
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
    ) -> anyhow::Result<(String, impl FnOnce() -> anyhow::Result<()> + Send + 'static)> {
        let mounted = self
            .mounted()
            .into_iter()
            .find(|m| m.remote.point == point)
            .ok_or_else(|| anyhow::anyhow!("no remote folder is mounted there"))?;
        let connector = mounted
            .connector
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Skwir does not know the tool that mounted it"))?;
        let label = mounted.recognised.label.clone();
        if !force {
            under_way(app, point)
                .map_err(|e| anyhow::anyhow!("“{label}” was not disconnected: {e:#}"))?;
        }
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/"));
        app.release(point, &Location::from_path(home))?;
        let key = point.display().to_string();
        self.state.lock().busy.insert(key.clone(), "Disconnecting…");
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
            }
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
                if result.is_ok() {
                    app.notify(NotifyLevel::Info, format!("Disconnected from “{said}”"));
                }
            }
            result
        };
        Ok((label, letting_go))
    }
}

/// Refused while actions of Skwir's are under way at or below `point`
/// (`Executor::under_way_in`), named: letting go of the mount would cut
/// them.
fn under_way(app: &App, point: &Path) -> anyhow::Result<()> {
    let named: Vec<String> = app
        .executor
        .under_way_in(point)
        .into_iter()
        .map(|n| format!("“{n}”"))
        .collect();
    match named.as_slice() {
        [] => Ok(()),
        [one] => anyhow::bail!("{one} is under way there"),
        [rest @ .., last] => anyhow::bail!("{} and {last} are under way there", rest.join(", ")),
    }
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
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String> {
        self.noted().secret(title, accept, cancel)
    }
    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
        self.noted().confirm(title, detail, cancel)
    }
    fn password(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<Password> {
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
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String> {
        self.asked(title, accept, false, cancel)
            .map(|answer| answer.text)
    }

    fn password(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<Password> {
        self.asked(title, accept, true, cancel)
            .map(|answer| Password {
                text: answer.text,
                remember: answer.remember,
            })
    }

    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
        self.confirmed(title, detail, cancel)
    }
}

impl WindowAsk {
    /// Ask the window's masked prompt, offering Remember where `password`.
    fn asked(
        &self,
        title: &str,
        accept: &str,
        password: bool,
        cancel: &CancelToken,
    ) -> Option<skwir_core::app::SecretAnswer> {
        let app = self.app.upgrade()?;
        if !app.window_shown() {
            return None;
        }
        let (id, answer) = if password {
            app.ask_password(title, accept)
        } else {
            app.ask_secret(title, accept)
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

    fn confirmed(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
        let Some(app) = self.app.upgrade() else {
            return false;
        };
        if !app.window_shown() {
            return false;
        }
        let (id, answer) = app.ask_as(skwir_core::app::Asker::Connection, title, detail);
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

    #[test]
    fn kde_s_desktop_tries_kio_before_gvfs() {
        let on = |desktops: &[&str]| {
            super::order_on(&desktops.iter().map(|d| (*d).to_owned()).collect::<Vec<_>>())
        };
        // Off Linux, the system's own shares first, whatever the desktop.
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(on(&["KDE"]), "system rclone sshfs fstab gvfs kio");
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
        fn title(&self) -> &'static str {
            "slow"
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Sftp]
        }
        fn unavailable(&self) -> Option<String> {
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
        fn secret(&self, _: &str, _: &str, _: &CancelToken) -> Option<String> {
            None
        }
        fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
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
        fn title(&self) -> &'static str {
            self.id
        }
        fn schemes(&self) -> &'static [Scheme] {
            &[Scheme::Nfs]
        }
        fn unavailable(&self) -> Option<String> {
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
                ask.confirm("Trust it?", "", cancel);
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
        assert_eq!(format!("{error:#}"), "first: no export; second: no entry");
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
}
