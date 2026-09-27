//! bibli plugin: map-tiles. What maps are drawn on (ADR-020): the outline
//! of the land, drawn here from Natural Earth and never downloaded, until
//! the user chooses OpenStreetMap or a server of their own. It is the only
//! code in bibli that uses the network, and only after that choice;
//! disabling it withdraws every source and cancels the downloads in flight.
mod download;
mod outline;

use std::sync::{Arc, Weak};

use bibli_core::CancelToken;
use bibli_core::app::App;
use bibli_core::plugin::Plugin;
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::tiles::{SourceInfo, TileDone, TileRequest, TileSource};
use parking_lot::Mutex;

const ID: &str = "map-tiles";
pub const SOURCE_KEY: &str = "map-tiles.source";
pub const URL_KEY: &str = "map-tiles.url";
pub const ATTRIBUTION_KEY: &str = "map-tiles.attribution";
/// The deepest zoom a tile server is asked for.
const MAX_ZOOM: u8 = 19;
const OSM_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

#[derive(Default)]
pub struct MapTilesPlugin {
    source: Mutex<Option<Arc<Source>>>,
}

impl Plugin for MapTilesPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Map backgrounds"
    }
    fn description(&self) -> &'static str {
        "Draw maps on the outline of the land, offline, or on OpenStreetMap or another tile server once you choose one. Only this plugin uses the network"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec::choice(
            SOURCE_KEY,
            "Map background",
            "What maps are drawn on. The outline needs no network. The others download the tiles you look at, which tells their server where you look",
            "outline",
            &[
                ("outline", "Outline of the land (offline)"),
                ("openstreetmap", "OpenStreetMap (downloads tiles)"),
                ("custom", "Another tile server"),
            ],
        ));
        app.settings.declare(SettingSpec::text(
            URL_KEY,
            "Tile server address",
            "For another tile server: its address, with {z}, {x} and {y} where the tile goes, such as https://tiles.example.org/{z}/{x}/{y}.png",
            "",
        ));
        app.settings.declare(SettingSpec::text(
            ATTRIBUTION_KEY,
            "Tile server attribution",
            "For another tile server: what its licence asks to show on the map under its tiles",
            "",
        ));
        let source = Arc::new(Source {
            app: app.weak(),
            scheduler: app.scheduler.clone(),
            outline: Arc::default(),
            downloads: Arc::new(download::Downloads::new(
                app.cache.namespace(ID),
                CancelToken::new(),
            )),
        });
        app.tiles
            .register(ID, Arc::clone(&source) as Arc<dyn TileSource>);
        if let Some(old) = self.source.lock().replace(source) {
            old.downloads.stop();
        }
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key.starts_with("map-tiles.")) {
            app.tiles.changed();
        }
    }
    fn on_disable(&self, _app: &App) {
        if let Some(source) = self.source.lock().take() {
            source.downloads.stop();
        }
    }
}

/// Where tiles come from, as the settings say.
#[derive(Debug, PartialEq)]
enum Choice {
    Outline,
    Server {
        template: String,
        attribution: String,
        url: Option<String>,
    },
}

fn choose(source: Option<&str>, template: Option<String>, attribution: Option<String>) -> Choice {
    match source {
        Some("openstreetmap") => Choice::Server {
            template: OSM_URL.into(),
            attribution: "© OpenStreetMap contributors".into(),
            url: Some("https://www.openstreetmap.org/copyright".into()),
        },
        Some("custom") => Choice::Server {
            template: template.unwrap_or_default().trim().to_owned(),
            attribution: attribution.unwrap_or_default().trim().to_owned(),
            url: None,
        },
        _ => Choice::Outline,
    }
}

/// The address of a tile on a server whose addresses look like `template`.
fn address(template: &str, request: &TileRequest) -> anyhow::Result<String> {
    anyhow::ensure!(
        template.starts_with("https://") || template.starts_with("http://"),
        "The tile server address must start with https://"
    );
    anyhow::ensure!(
        ["{z}", "{x}", "{y}"]
            .iter()
            .all(|part| template.contains(part)),
        "The tile server address needs {{z}}, {{x}} and {{y}}"
    );
    Ok(template
        .replace("{z}", &request.z.to_string())
        .replace("{x}", &request.x.to_string())
        .replace("{y}", &request.y.to_string()))
}

struct Source {
    app: Weak<App>,
    scheduler: bibli_core::scheduler::Scheduler,
    /// Decoded the first time a map needs it.
    outline: Arc<Mutex<Option<Arc<outline::Outline>>>>,
    downloads: Arc<download::Downloads>,
}

impl Source {
    fn choice(&self) -> Choice {
        let Some(app) = self.app.upgrade() else {
            return Choice::Outline;
        };
        let text = |key| app.settings.get::<String>(key);
        choose(
            text(SOURCE_KEY).as_deref(),
            text(URL_KEY),
            text(ATTRIBUTION_KEY),
        )
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
                max_zoom: MAX_ZOOM,
                detailed: false,
                setting: Some(SOURCE_KEY.into()),
            },
            Choice::Server {
                attribution, url, ..
            } => SourceInfo {
                attribution,
                attribution_url: url,
                max_zoom: MAX_ZOOM,
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
            Choice::Server { template, .. } => {
                let url = match address(&template, &request) {
                    Ok(url) => url,
                    Err(error) => return done(Err(error)),
                };
                let downloads = Arc::clone(&self.downloads);
                self.scheduler
                    .spawn(Priority::Visible, "map-tile", cancel.clone(), move || {
                        downloads.get(url, cancel, done);
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::tiles::Palette;

    #[test]
    fn the_settings_choose_where_tiles_come_from() {
        assert_eq!(choose(None, None, None), Choice::Outline);
        let Choice::Server {
            template,
            attribution,
            ..
        } = choose(Some("openstreetmap"), Some("ignored".into()), None)
        else {
            panic!("a server");
        };
        assert_eq!(template, OSM_URL);
        assert!(attribution.contains("OpenStreetMap contributors"));
    }

    #[test]
    fn a_tile_address_fills_the_template() {
        let request = TileRequest {
            z: 12,
            x: 2100,
            y: 1460,
            scale: 1,
            palette: Palette::default(),
        };
        assert_eq!(
            address(OSM_URL, &request).unwrap(),
            "https://tile.openstreetmap.org/12/2100/1460.png"
        );
        assert!(address("file:///etc/{z}/{x}/{y}", &request).is_err());
        assert!(address("https://example.org/tile.png", &request).is_err());
    }

    #[test]
    fn the_outline_answers_without_a_network() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(bibli_core::app::AppConfig::ephemeral(dir.path())).unwrap();
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
