//! Remote folders (ADR-077): an address (`sftp://`, `smb://`, `dav://`,
//! `ftp://`, `nfs://`, `afp://`, `rclone://`) opens by connecting it, through
//! an installed tool that mounts it, then showing the mounted folder. A
//! remote folder is a folder: everything works there. This plugin knows the
//! tools (connectors), names each mount in the sidebar's Network section,
//! lets go of them, and says what is shown of each unasked (thumbnails).
//!
//! Settings: `remote.connectors` (the tools, in the order tried),
//! `remote.thumbnails` (for a remote the user chose nothing for),
//! `remote.chosen` (what the user chose for each, by address).

pub mod address;
pub mod connector;
pub mod fstab;
pub mod gio_copy;
pub mod gvfs;
mod gvfs_spec;
mod known;
pub mod lan;
mod mounter;
mod provider;
pub mod rclone;
pub mod rsync;
pub mod sshfs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::fs::reach::{self, Known, Remote, Thumbnails};
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::Place;
use skwir_core::scheduler::Priority;
use skwir_core::settings::SettingSpec;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connector, Recognised, Server};

pub const ID: &str = "remote";

/// The sidebar's section of remotes.
pub const GROUP: &str = "network";

/// The sidebar's section of this machine's devices, where a phone or a
/// camera GVfs reads is.
const DEVICES: &str = "devices";

const CONNECTORS: &str = "remote.connectors";
const THUMBNAILS: &str = "remote.thumbnails";
const CHOSEN: &str = "remote.chosen";

/// The tools, in the order the user chose by default (2026-10-20: sshfs
/// first for SFTP, then GVfs, rclone, the system's mounts).
const ORDER: &str = "sshfs gvfs rclone fstab";

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
    /// Each mount's free and whole space, as its server says (measured
    /// in its lane).
    space: HashMap<PathBuf, (u64, u64)>,
    /// Bumped by each refresh, so a slower one never wins.
    generation: u64,
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
}

impl Default for Remotes {
    fn default() -> Self {
        Remotes::with(vec![
            Arc::new(sshfs::Sshfs::default()),
            Arc::new(gvfs::Gvfs::default()),
            Arc::new(rclone::Rclone),
            Arc::new(fstab::Fstab),
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
        }
    }

    fn lifetime(&self) -> Option<CancelToken> {
        self.state.lock().lifetime.clone()
    }

    /// The connectors in the order the setting says, those it leaves out
    /// after, in their own.
    fn ordered(&self, app: &App) -> Vec<Arc<dyn Connector>> {
        let order: String = app
            .settings
            .get(CONNECTORS)
            .unwrap_or_else(|| ORDER.to_owned());
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
    /// GVfs's one by one. Lists GVfs's bridge: on a worker.
    fn mounted(&self) -> Vec<Mounted> {
        let mut remotes = Vec::new();
        for remote in reach::all() {
            if remote.fstype == gvfs::BRIDGE && remote.device.is_some() {
                // Each folder of the bridge is a mount of GVfs's; the
                // bridge answers its list from memory.
                if let Ok(folders) = std::fs::read_dir(&remote.point) {
                    remotes.extend(
                        folders
                            .flatten()
                            .filter_map(|folder| reach::of(&folder.path())),
                    );
                }
                continue;
            }
            remotes.push(remote);
        }
        remotes
            .into_iter()
            .map(|remote| {
                let known = self
                    .connectors
                    .iter()
                    .find_map(|c| Some((Arc::clone(c), c.recognises(&remote)?)));
                match known {
                    Some((connector, recognised)) => Mounted {
                        remote,
                        connector: Some(connector),
                        recognised,
                    },
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
                }
            })
            .collect()
    }

    /// Where `address` is on this machine: in a mount there already, else
    /// in one the first connector able to opens. Blocks (it may ask the
    /// user): on a thread of its own.
    ///
    /// # Errors
    /// Why it could not be reached, in the user's words.
    pub fn reach(
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
                match std::fs::symlink_metadata(&mounted.remote.point) {
                    Err(error) if lost(&error) => {
                        let _ = mounter::force_unmount(&mounted.remote.point);
                        break;
                    }
                    _ => return Ok(join(&mounted.remote.point, below)),
                }
            }
        }
        let mut why_not = Vec::new();
        for connector in self.ordered(app) {
            if !connector.schemes().contains(&address.scheme) {
                continue;
            }
            if let Some(reason) = connector.unavailable() {
                why_not.push(reason);
                continue;
            }
            let connected = connector
                .connect(address, ask, cancel)
                .map_err(|error| anyhow::anyhow!("{error:#}"))?;
            reach::changed();
            let below = connected.address.below(address).unwrap_or_default();
            let path = join(&connected.point, below);
            self.refresh(app);
            return Ok(path);
        }
        if why_not.is_empty() {
            anyhow::bail!(
                "no tool Skwir knows opens {} addresses",
                address.scheme.title()
            );
        }
        anyhow::bail!("nothing installed can open it: {}", why_not.join("; "))
    }

    /// Read again what is mounted and what servers are known, then
    /// publish: on a worker.
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
                let mounted = this.mounted();
                let mut servers = Vec::new();
                for connector in this.connectors.iter() {
                    if connector.unavailable().is_none() {
                        servers.extend(connector.servers(&lifetime));
                    }
                }
                // Whether each server is near, for its thumbnails.
                let hosts: Vec<String> = mounted
                    .iter()
                    .filter_map(|m| Some(m.recognised.address.as_ref()?.host.clone()))
                    .collect();
                let mut local = this.state.lock().local.clone();
                for host in hosts {
                    local
                        .entry(host.clone())
                        .or_insert_with(|| lan::local(&host));
                }
                {
                    let mut state = this.state.lock();
                    if state.generation != generation {
                        return;
                    }
                    state.servers = servers;
                    state.local = local;
                    state
                        .lost
                        .retain(|point| mounted.iter().any(|m| m.remote.point == *point));
                    state
                        .space
                        .retain(|point, _| mounted.iter().any(|m| m.remote.point == *point));
                }
                let Some(app) = weak.upgrade() else {
                    return;
                };
                this.publish(&app, &mounted);
                // Whether each mount's program still answers, asked in the
                // remote's own lane: a server that stopped answering holds
                // that alone (ADR-077).
                let mounted = Arc::new(mounted);
                for m in mounted.iter() {
                    let (this, weak, all) = (this.clone(), weak.clone(), Arc::clone(&mounted));
                    let point = m.remote.point.clone();
                    app.scheduler.spawn_in(
                        m.remote.lane(),
                        Priority::Background,
                        "remote-alive",
                        lifetime.clone(),
                        move || {
                            let gone = std::fs::symlink_metadata(&point).is_err_and(|e| lost(&e));
                            // How full it is, as the server says: asked
                            // here too, where waiting holds no one else.
                            let space = if gone {
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
                                this.publish(&app, &all);
                            }
                        },
                    );
                }
            },
        );
    }

    /// Put the mounts and the servers in the sidebar, and what is known of
    /// each mount where the core asks (`Registry::remotes`).
    fn publish(&self, app: &App, mounted: &[Mounted]) {
        let state = self.state.lock();
        let chosen: HashMap<String, String> = app.settings.get(CHOSEN).unwrap_or_default();
        let default: String = app
            .settings
            .get(THUMBNAILS)
            .unwrap_or_else(|| "automatic".to_owned());
        let mut places = Vec::new();
        let mut known = Vec::new();
        for m in mounted {
            let point = &m.remote.point;
            let address = m.recognised.address.as_ref();
            let key = address.map_or_else(|| point.display().to_string(), ToString::to_string);
            let rule = chosen.get(&key).unwrap_or(&default);
            let near = if m.recognised.device.is_some() {
                Some(true)
            } else {
                address.and_then(|a| state.local.get(&a.host).copied().flatten())
            };
            known.push(Known {
                point: point.clone(),
                address: address.map(Address::location),
                label: m.recognised.label.clone(),
                thumbnails: thumbnails(rule, near),
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
            places.push(place);
        }
        drop(state);
        app.registry.places.replace_owner(ID, places);
        app.registry.remotes.replace_owner(ID, known);
        app.wake();
    }
}

/// Whether the user may let go of a mount: Skwir's, GVfs's, one fstab lets
/// them mount.
fn can_disconnect(connector: &dyn Connector, remote: &Remote, recognised: &Recognised) -> bool {
    match connector.id() {
        "fstab" => fstab::user_mountable(&remote.point),
        "sshfs" | "rclone" => recognised.ours || cfg!(target_os = "linux"),
        _ => true,
    }
}

/// What a remote's thumbnails follow: the rule chosen (`never`, `small`,
/// `always`, `automatic`), Automatic being Always on the local network and
/// Never where that is not known (ADR-077).
fn thumbnails(rule: &str, near: Option<bool>) -> Thumbnails {
    match rule {
        "never" => Thumbnails::Never,
        "small" => Thumbnails::Small,
        "always" => Thumbnails::Always,
        _ => match near {
            Some(true) => Thumbnails::Always,
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
            if value == rule {
                format!("{title} ✓")
            } else {
                title.to_owned()
            },
            "",
            order,
            Invocation::new("remote.thumbnails")
                .with("remote", key)
                .with("rule", value),
        );
        item.submenu = Some("Thumbnails".to_owned());
        item
    })
    .collect()
}

/// Whether `error` says a FUSE program is gone (`ENOTCONN`).
fn lost(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotConnected
}

/// `point` joined with `below`, the bytes of a server's path.
fn join(point: &Path, below: &[u8]) -> PathBuf {
    if below.is_empty() {
        return point.to_path_buf();
    }
    point.join(skwir_core::os_path::from_bytes(below.to_vec()))
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
        "Open SFTP, SMB, WebDAV, FTP, NFS and rclone addresses as folders, through sshfs, GVfs, rclone or the system's mounts"
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
        ]
    }

    fn on_disable(&self, _app: &App) {
        if let Some(lifetime) = self.remotes.state.lock().lifetime.take() {
            lifetime.cancel();
        }
    }

    fn register(&self, app: &App) {
        for transfer in self.remotes.connectors.iter().filter_map(|c| c.transfer()) {
            app.transfers.register(ID, transfer);
        }
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
                "The tools that mount an sftp://, smb:// or other address, tried in this order among those installed that open it: sshfs, gvfs, rclone, fstab.",
                ORDER,
            )
            .page(skwir_core::settings::page::GENERAL)
            .section("Remote folders"),
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
        let remotes = self.remotes.clone();
        app.commands.register::<ConnectArgs, _>(
            ID,
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
            "remote.disconnect",
            "Disconnect",
            "Let go of a remote folder: what shows it goes home",
            move |args, app| {
                remotes.disconnect(app, args.path.into_inner(), args.force)?;
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands.register::<ThumbnailsArgs, _>(
            ID,
            "remote.thumbnails",
            "Thumbnails of a remote",
            "Choose when a remote folder's files show thumbnails: automatic, never, small or always",
            move |args, app| {
                anyhow::ensure!(
                    ["automatic", "never", "small", "always"].contains(&args.rule.as_str()),
                    "a rule is automatic, never, small or always"
                );
                let (remotes, weak) = (remotes.clone(), app.weak());
                // Kept in the settings file, written on a worker.
                app.scheduler.spawn(
                    skwir_core::scheduler::Priority::Normal,
                    "remote.thumbnails",
                    app.lifetime().child(),
                    move || {
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
                    },
                );
                Ok(Outcome::Done)
            },
        );
        let remotes = self.remotes.clone();
        app.commands
            .register::<skwir_core::command::NoArgs, _>(
                ID,
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
        if keys.iter().any(|k| k == THUMBNAILS || k == CHOSEN) {
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
        known.extend(self.state.lock().servers.iter().map(|s| s.address.clone()));
        known.extend(
            skwir_platform::places::remote_bookmarks()
                .into_iter()
                .filter_map(|b| Address::parse(&b.address).ok()),
        );
        known
    }

    /// Let go of the mount at `point`: what shows it goes home, then its
    /// tool lets go of it, on a worker.
    fn disconnect(&self, app: &App, point: PathBuf, force: bool) -> anyhow::Result<()> {
        let mounted = self
            .mounted()
            .into_iter()
            .find(|m| m.remote.point == point)
            .ok_or_else(|| anyhow::anyhow!("no remote folder is mounted there"))?;
        let connector = mounted
            .connector
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Skwir does not know the tool that mounted it"))?;
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/"));
        app.release(&point, &Location::from_path(home))?;
        let key = point.display().to_string();
        self.state.lock().busy.insert(key.clone(), "Disconnecting…");
        self.refresh(app);
        let this = self.clone();
        let lifetime = self.lifetime().unwrap_or_default();
        let weak = app.weak();
        let label = mounted.recognised.label.clone();
        std::thread::Builder::new()
            .name("skwir-disconnect".into())
            .spawn(move || {
                // What showed it lets go first: the folders left close on a
                // worker a moment later, and what reads it in its lane ends;
                // until then the system refuses the unmount as busy.
                if let Some(app) = weak.upgrade() {
                    let (lane, until) = (mounted.remote.lane(), Instant::now() + LET_GO);
                    while Instant::now() < until {
                        let left = skwir_core::fs::wait_let_go(
                            &mounted.remote.point,
                            until.saturating_duration_since(Instant::now()),
                        );
                        if left && app.scheduler.stalled(lane).is_none() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                let mut result =
                    connector.disconnect(&mounted.remote, &mounted.recognised, &lifetime);
                if result.is_err() && force && !mounted.remote.by_kernel() {
                    result = mounter::force_unmount(&point);
                }
                this.state.lock().busy.remove(&key);
                let Some(app) = weak.upgrade() else { return };
                this.refresh(&app);
                match result {
                    Ok(()) => app.notify(NotifyLevel::Info, format!("Disconnected from “{label}”")),
                    Err(error) => app.notify(
                        NotifyLevel::Error,
                        format!("“{label}” was not disconnected: {error:#}"),
                    ),
                }
            })?;
        Ok(())
    }
}

/// How long a question waits between looks at whether its connection was
/// cancelled.
const LOOK: Duration = Duration::from_millis(100);

/// How long letting go of a mount waits for what showed it and what read
/// it there to let go first.
const LET_GO: Duration = Duration::from_secs(10);

/// What a connection asks of the window: a password in its masked
/// prompt, a server it cannot vouch for as a question.
pub struct WindowAsk {
    pub app: std::sync::Weak<App>,
}

impl Ask for WindowAsk {
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String> {
        let app = self.app.upgrade()?;
        if !app.window_shown() {
            return None;
        }
        let (id, answer) = app.ask_secret(title, accept);
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

    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
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
