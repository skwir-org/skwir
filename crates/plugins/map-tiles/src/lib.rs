//! skwir plugin: map-tiles. What maps are drawn on (ADR-020): the outline
//! of the land, drawn here from Natural Earth and never downloaded, until
//! the user chooses OpenStreetMap or a server of their own. It is the only
//! code in skwir that uses the network, and only after that choice;
//! disabling it withdraws every source and cancels the downloads in flight.
//!
//! Tiles kept in a file (`PMTiles`, `MBTiles`: `archive.rs`) are previewed as
//! the map they hold (`preview.rs`), read from the file alone.
mod archive;
mod download;
mod http;
mod mbtiles;
mod outline;
mod pmtiles;
mod preview;
mod server;

use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use server::Server;
use skwir_core::CancelToken;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::Outcome;
use skwir_core::plugin::Plugin;
use skwir_core::scheduler::Priority;
use skwir_core::settings::{RecordField, SettingSpec};
use skwir_core::tiles::{Palette, SourceInfo, TileDone, TileRequest, TileSource};

const ID: &str = "map-tiles";
pub const SOURCE_KEY: &str = "map-tiles.source";
/// The user's tile servers (`server::Server`), each a record.
pub const SERVERS_KEY: &str = "map-tiles.servers";
/// The field of a server's record that says its API key is in the keyring.
pub const KEY_FIELD: &str = "key";
/// A choice of `SOURCE_KEY` naming one of the servers: `server:<id>`.
const SERVER_CHOICE: &str = "server:";
const OUTLINE: &str = "outline";

#[derive(Default)]
pub struct MapTilesPlugin {
    source: Mutex<Option<Arc<Source>>>,
    /// Ends with the plugin: tile archives previewed are no longer read.
    previews: Mutex<Option<CancelToken>>,
}

/// The servers the settings hold.
fn servers(app: &App) -> Vec<Server> {
    app.settings
        .get::<toml::Value>(SERVERS_KEY)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Server::from_record)
        .collect()
}

/// The background choice, with the servers the user described: declared
/// again whenever they change, so the dialog and the map's picker offer
/// the same list.
fn declare_source(app: &App) {
    let servers = servers(app);
    let mut choices: Vec<(String, String)> = vec![
        (OUTLINE.into(), "Outline".into()),
        (server::OSM_ID.into(), "OpenStreetMap".into()),
    ];
    choices.extend(
        servers
            .iter()
            .map(|s| (format!("{SERVER_CHOICE}{}", s.id), s.name.clone())),
    );
    let choices: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    app.settings.declare(SettingSpec::choice(
        SOURCE_KEY,
        "Map background",
        "What maps are drawn on. The outline of the land needs no network. OpenStreetMap and other servers download the tiles you look at, which tells them where you look.",
        OUTLINE,
        &choices,
    )
    .more_choices("the tile servers you add"));
}

/// The chosen background was a server just removed: the outline again,
/// so the choice shows what the maps are drawn on. Runs on the save's
/// worker, after the settings were written.
fn forget_removed(app: &App) {
    let chosen = app.settings.get::<String>(SOURCE_KEY).unwrap_or_default();
    let Some(id) = chosen.strip_prefix(SERVER_CHOICE) else {
        return;
    };
    if servers(app).iter().any(|s| s.id == id) {
        return;
    }
    app.settings
        .set(SOURCE_KEY, toml::Value::String(OUTLINE.into()));
    if let Err(error) = app.settings.save() {
        tracing::warn!("map tiles: cannot go back to the outline: {error:#}");
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TestArgs {
    /// The server's id, as its record gives it.
    id: String,
}

impl Plugin for MapTilesPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Map backgrounds"
    }
    fn description(&self) -> &'static str {
        "Draw maps on the outline of the land, offline, or on OpenStreetMap or tile servers you add once you choose one, and preview maps kept in files (PMTiles, MBTiles). Only this plugin uses the network"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Network])
    }
    fn register(&self, app: &App) {
        app.settings.declare(
            SettingSpec::records(
                SERVERS_KEY,
                "Tile servers",
                "Backgrounds from other servers, to choose in Map background or from a map.",
                "tile server",
                vec![
                    RecordField::text("name", "Name", "What the server is called in the list", "Topographic").required(),
                    RecordField::text(
                        "url",
                        "Address",
                        "Where a tile is: {z}, {x} and {y} (or {-y}, counted from the bottom), or {q} for a quadkey; {s} for a subdomain, {r} for @2x on a dense screen, {key} for the API key",
                        "https://tiles.example.org/{z}/{x}/{y}.png",
                    )
                    .required(),
                    RecordField::text("subdomains", "Subdomains", "What {s} stands for, one after another", "a b c"),
                    RecordField::text("attribution", "Attribution", "What the server's licence asks to show on the map under its tiles", "© …"),
                    RecordField::integer("max_zoom", "Deepest zoom", "Closer views enlarge the tiles of this zoom", (0, 22)),
                    RecordField::pairs(
                        "headers",
                        "Headers",
                        "Sent with every request: User-Agent replaces skwir's, Referer, Authorization… {key} is replaced by the API key",
                    ),
                    RecordField::secret(KEY_FIELD, "API key", "Kept in your keyring, never in the settings. Put {key} in the address or a header where it goes"),
                ],
            )
            .record_action("Test", "map-tiles.test_server")
            // After the choice they add to: sections follow what has none.
            .section("Other servers")
            .validate_with(|value, _| server::check_servers(value)),
        );
        declare_source(app);
        forget_removed(app);
        let source = Arc::new(Source {
            app: app.weak(),
            scheduler: app.scheduler.clone(),
            outline: Arc::default(),
            downloads: Arc::new(download::Downloads::new(
                app.cache.namespace(ID),
                CancelToken::new(),
            )),
        });
        let tester = Arc::clone(&source);
        app.commands.register::<TestArgs, _>(
            ID,
            "map-tiles.test_server",
            "Test a tile server",
            "Ask a tile server for one tile, now, and say what it answered",
            move |args, app| {
                let server = servers(app)
                    .into_iter()
                    .find(|s| s.id == args.id)
                    .ok_or_else(|| anyhow::anyhow!("No tile server {}", args.id))?;
                tester.test(Arc::new(server), app);
                Ok(Outcome::Done)
            },
        );
        app.tiles
            .register(ID, Arc::clone(&source) as Arc<dyn TileSource>);
        if let Some(old) = self.source.lock().replace(source) {
            old.downloads.stop();
        }
        let lifetime = app.lifetime().child();
        if let Some(old) = self.previews.lock().replace(lifetime.clone()) {
            old.cancel();
        }
        app.previews.register(
            ID,
            Arc::new(preview::Provider {
                scheduler: app.scheduler.clone(),
                lifetime,
            }),
        );
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == SERVERS_KEY) {
            declare_source(app);
            forget_removed(app);
            if let Some(source) = self.source.lock().as_ref() {
                source.downloads.forget_keys();
            }
        }
        if keys.iter().any(|key| key.starts_with("map-tiles.")) {
            app.tiles.changed();
        }
    }
    fn on_disable(&self, _app: &App) {
        if let Some(source) = self.source.lock().take() {
            source.downloads.stop();
        }
        if let Some(previews) = self.previews.lock().take() {
            previews.cancel();
        }
    }
}

/// Where tiles come from, as the settings say.
#[derive(Debug, PartialEq)]
enum Choice {
    Outline,
    Server(Arc<Server>),
}

fn choose(source: Option<&str>, servers: Vec<Server>) -> Choice {
    match source {
        Some(server::OSM_ID) => Choice::Server(Arc::new(Server::openstreetmap())),
        Some(choice) if choice.starts_with(SERVER_CHOICE) => servers
            .into_iter()
            .find(|s| s.id == choice[SERVER_CHOICE.len()..])
            .map_or(Choice::Outline, |s| Choice::Server(Arc::new(s))),
        _ => Choice::Outline,
    }
}

struct Source {
    app: Weak<App>,
    scheduler: skwir_core::scheduler::Scheduler,
    /// Decoded the first time a map needs it.
    outline: Arc<Mutex<Option<Arc<outline::Outline>>>>,
    downloads: Arc<download::Downloads>,
}

impl Source {
    fn choice(&self) -> Choice {
        let Some(app) = self.app.upgrade() else {
            return Choice::Outline;
        };
        choose(
            app.settings.get::<String>(SOURCE_KEY).as_deref(),
            servers(&app),
        )
    }

    /// Ask `server` for the world's tile and say what it answered.
    fn test(&self, server: Arc<Server>, app: &App) {
        let weak = app.weak();
        let name = server.name.clone();
        let tile = TileRequest {
            z: 0,
            x: 0,
            y: 0,
            scale: 1,
            palette: Palette::default(),
        };
        self.downloads.ask(
            server,
            tile,
            app.lifetime().child(),
            Box::new(move |result| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(image) => app.notify(
                        NotifyLevel::Info,
                        format!(
                            "“{name}” answered with a tile of {}×{}",
                            image.width, image.height
                        ),
                    ),
                    Err(error) => app.notify_about_setting(
                        NotifyLevel::Error,
                        format!("“{name}” gave no tile: {error:#}"),
                        SERVERS_KEY,
                    ),
                }
            }),
        );
    }
}

fn outline(slot: &Mutex<Option<Arc<outline::Outline>>>) -> anyhow::Result<Arc<outline::Outline>> {
    let mut slot = slot.lock();
    if let Some(outline) = slot.as_ref() {
        return Ok(Arc::clone(outline));
    }
    let outline = Arc::new(outline::Outline::load()?);
    *slot = Some(Arc::clone(&outline));
    Ok(outline)
}

impl TileSource for Source {
    fn info(&self) -> SourceInfo {
        match self.choice() {
            Choice::Outline => SourceInfo {
                attribution: "Natural Earth".into(),
                attribution_url: Some("https://www.naturalearthdata.com".into()),
                max_zoom: server::MAX_ZOOM,
                min_zoom: 0,
                bounds: None,
                detailed: false,
                setting: Some(SOURCE_KEY.into()),
            },
            Choice::Server(server) => SourceInfo {
                attribution: server.attribution.clone(),
                attribution_url: server.attribution_url.clone(),
                max_zoom: server.max_zoom,
                min_zoom: 0,
                bounds: None,
                detailed: true,
                setting: Some(SOURCE_KEY.into()),
            },
        }
    }

    fn request(&self, request: TileRequest, cancel: CancelToken, done: TileDone) {
        match self.choice() {
            Choice::Outline => {
                let slot = Arc::clone(&self.outline);
                let token = cancel.clone();
                self.scheduler
                    .spawn(Priority::Visible, "map-tile", cancel, move || {
                        let tile = outline(&slot).and_then(|o| o.tile(&request));
                        if !token.is_cancelled() {
                            done(tile);
                        }
                    });
            }
            Choice::Server(server) => {
                let downloads = Arc::clone(&self.downloads);
                let ticket = downloads.ticket();
                self.scheduler
                    .spawn(Priority::Visible, "map-tile", cancel.clone(), move || {
                        downloads.get(ticket, server, request, cancel, done);
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::tiles::Palette;

    #[test]
    fn the_settings_choose_where_tiles_come_from() {
        assert_eq!(choose(None, Vec::new()), Choice::Outline);
        let Choice::Server(osm) = choose(Some("openstreetmap"), Vec::new()) else {
            panic!("a server");
        };
        assert!(osm.attribution.contains("OpenStreetMap contributors"));
        let mine = Server {
            id: "t".into(),
            ..Server::openstreetmap()
        };
        assert_eq!(
            choose(Some("server:t"), vec![mine.clone()]),
            Choice::Server(Arc::new(mine))
        );
        // A server removed since: the outline, not an error.
        assert_eq!(choose(Some("server:gone"), Vec::new()), Choice::Outline);
    }

    #[test]
    fn servers_become_choices_and_a_removed_one_chooses_the_outline() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(dir.path())).unwrap();
        let mut record = toml::Table::new();
        record.insert("id".into(), toml::Value::String("custom".into()));
        record.insert("name".into(), toml::Value::String("Custom server".into()));
        record.insert(
            "url".into(),
            toml::Value::String("https://x.org/{z}/{x}/{y}.png".into()),
        );
        app.settings.set(
            SERVERS_KEY,
            toml::Value::Array(vec![toml::Value::Table(record)]),
        );
        app.settings.set(
            SOURCE_KEY,
            toml::Value::String(format!("{SERVER_CHOICE}custom")),
        );
        MapTilesPlugin::default().register(&app);
        assert_eq!(servers(&app).len(), 1);
        assert_eq!(
            app.settings.get::<String>(SOURCE_KEY).as_deref(),
            Some("server:custom")
        );
        let spec = app
            .settings
            .schema()
            .into_iter()
            .find(|s| s.key == SOURCE_KEY)
            .unwrap();
        assert_eq!(spec.choice_label("server:custom"), "Custom server");
        // A record the server check refuses never reaches the settings.
        let servers_spec = app
            .settings
            .schema()
            .into_iter()
            .find(|s| s.key == SERVERS_KEY)
            .unwrap();
        assert!(
            servers_spec
                .parse(r#"[{"id":"b","name":"B","url":"https://x.org/{z}/{x}"}]"#)
                .is_err()
        );
        assert!(
            servers_spec
                .parse(
                    r#"[{"id":"b","name":"B","url":"https://x.org/{z}/{x}/{y}","key":"hunter2"}]"#
                )
                .is_err()
        );
        assert!(servers_spec.parse(r#"[{"id":"b","name":"B","url":"https://x.org/{z}/{x}/{y}?k={key}","key":"keyring"}]"#).is_ok());
        // Removing the chosen server chooses the outline again.
        app.settings
            .set(SERVERS_KEY, toml::Value::Array(Vec::new()));
        MapTilesPlugin::default().on_settings_changed(&app, &[SERVERS_KEY.to_owned()]);
        assert_eq!(
            app.settings.get::<String>(SOURCE_KEY).as_deref(),
            Some(OUTLINE)
        );
    }

    #[test]
    fn the_outline_answers_without_a_network() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(dir.path())).unwrap();
        let plugin = MapTilesPlugin::default();
        plugin.register(&app);
        let (source, lifetime) = app.tiles.source().unwrap();
        assert!(!source.info().detailed);
        let (tx, rx) = std::sync::mpsc::channel();
        source.request(
            TileRequest {
                z: 0,
                x: 0,
                y: 0,
                scale: 1,
                palette: Palette::default(),
            },
            lifetime.child(),
            Box::new(move |tile| {
                let _ = tx.send(tile.map(|image| image.width));
            }),
        );
        let width = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .unwrap();
        assert_eq!(width, 256);
        plugin.on_disable(&app);
        app.unregister_owner(ID);
        assert!(app.tiles.source().is_none());
        assert!(lifetime.is_cancelled());
    }
}
