//! The presence in the tray: an item as KDE's `StatusNotifierItem`
//! specification has it, at `/StatusNotifierItem` under a name of its own,
//! its menu as `com.canonical.dbusmenu` at `/MenuBar`, served on the
//! program's session connection while shown and taken back with it.
//! Hosts (Plasma's tray, waybar, GNOME's `AppIndicator` extension) read its
//! properties when told they changed, and call back on the bus's own
//! thread: what they ask is told at once through the caller's callback.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use zbus::blocking::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, StructureBuilder, Value};

use super::super::icon::{Badge, Icons};
use super::super::{Asked, OnAsked, Presence, Standing};
use super::{WATCHER, call, unique};

pub(super) const ITEM_PATH: &str = "/StatusNotifierItem";
pub(super) const MENU_PATH: &str = "/MenuBar";
/// The sizes the icon is offered at: hosts pick the nearest, and scale.
const SIZES: [u16; 4] = [16, 22, 32, 48];

/// The menu's entries, by id; 0 is the menu itself.
const OPEN: i32 = 1;
const SEPARATOR: i32 = 2;
const PAUSE: i32 = 3;
const STOP: i32 = 4;
const ENTRIES: [i32; 4] = [OPEN, SEPARATOR, PAUSE, STOP];

/// An icon as the specification carries it: width, height, ARGB32 in
/// network byte order.
type Pixmap = (i32, i32, Vec<u8>);

/// What hosts read of the item and its menu.
struct Look {
    id: String,
    title: String,
    status: &'static str,
    pixmaps: Vec<Pixmap>,
    badge: Badge,
    /// The tooltip's title and text, escaped: hosts read them as markup.
    tip: (String, String),
    pause: Option<bool>,
    /// The menu's layout's revision, counted up as it changes.
    revision: u32,
    /// The activation token the host gave last, for the next click.
    token: Option<String>,
}

/// The item and its menu, served under a name of their own.
pub(super) struct Served {
    bus: Connection,
    /// The connection's unique name: a bus started again has another.
    on: Option<String>,
    name: String,
    look: Arc<Mutex<Look>>,
    /// The watcher's owner it registered with, if any.
    watcher: Option<String>,
    /// What this served, to take back no more than it.
    item: bool,
    menu: bool,
    named: bool,
}

impl Served {
    /// The item and its menu served on `bus` under `name`, showing
    /// `presence`, its menu's choices told to `asked`; not registered yet.
    pub(super) fn on(
        bus: &Connection,
        name: String,
        (id, title): (&str, &str),
        presence: &Presence,
        icons: &mut Icons,
        asked: &OnAsked,
    ) -> zbus::Result<Served> {
        let badge = Badge::of(presence);
        let look = Arc::new(Mutex::new(Look {
            id: id.to_owned(),
            title: title.to_owned(),
            status: status(presence.standing),
            pixmaps: pixmaps(icons, badge),
            badge,
            tip: tip(presence),
            pause: presence.pause,
            revision: 1,
            token: None,
        }));
        let mut served = Served {
            bus: bus.clone(),
            on: unique(bus),
            name,
            look: Arc::clone(&look),
            watcher: None,
            item: false,
            menu: false,
            named: false,
        };
        // Dropped half made, what it served is taken back.
        let objects = bus.object_server();
        served.item = objects.at(
            ITEM_PATH,
            Item {
                look: Arc::clone(&look),
                asked: Arc::clone(asked),
            },
        )?;
        served.menu = objects.at(
            MENU_PATH,
            Menu {
                look,
                asked: Arc::clone(asked),
            },
        )?;
        if !served.item || !served.menu {
            return Err(zbus::Error::Failure(
                "another item is served on this connection".into(),
            ));
        }
        bus.request_name(served.name.as_str())?;
        served.named = true;
        Ok(served)
    }

    /// Whether this is served on `bus`, which did not start again since.
    pub(super) fn is_on(&self, bus: &Connection) -> bool {
        !self.bus.inner().is_closed() && self.on == unique(bus)
    }

    /// The watcher's owner it registered with.
    pub(super) fn watcher(&self) -> Option<&str> {
        self.watcher.as_deref()
    }

    /// Register with the watcher, which `watcher` owns: whether it took it.
    pub(super) fn register(&mut self, watcher: String) -> bool {
        match call(
            &self.bus,
            WATCHER,
            "RegisterStatusNotifierItem",
            &self.name.as_str(),
        ) {
            Ok(_) => {
                self.watcher = Some(watcher);
                true
            }
            Err(error) => {
                tracing::debug!("the tray's watcher refused the program's item: {error}");
                self.watcher = None;
                false
            }
        }
    }

    /// Show `presence`: hosts told what changed, the icon drawn again only
    /// when its badge changed.
    pub(super) fn show(&mut self, presence: &Presence, icons: &mut Icons) {
        let badge = Badge::of(presence);
        let (status, tip) = (status(presence.standing), tip(presence));
        let redraw = self.look.lock().badge != badge;
        // Drawn without holding what hosts read.
        let pixmaps = redraw.then(|| pixmaps(icons, badge));
        let (new_status, new_tip, revision) = {
            let mut look = self.look.lock();
            if let Some(pixmaps) = pixmaps {
                look.pixmaps = pixmaps;
                look.badge = badge;
            }
            let new_status = std::mem::replace(&mut look.status, status) != status;
            let new_tip = look.tip != tip;
            look.tip = tip;
            let revision = (look.pause != presence.pause).then(|| {
                look.pause = presence.pause;
                look.revision += 1;
                look.revision
            });
            (new_status, new_tip, revision)
        };
        let told = zbus::block_on(async {
            let item = SignalEmitter::new(self.bus.inner(), ITEM_PATH)?;
            if redraw {
                Item::new_icon(&item).await?;
                Item::new_attention_icon(&item).await?;
            }
            if new_tip {
                Item::new_tool_tip(&item).await?;
            }
            if new_status {
                Item::new_status(&item, status).await?;
            }
            if let Some(revision) = revision {
                let menu = SignalEmitter::new(self.bus.inner(), MENU_PATH)?;
                Menu::layout_updated(&menu, revision, 0).await?;
            }
            zbus::Result::Ok(())
        });
        if let Err(error) = told {
            tracing::debug!("the tray was not told the program's item changed: {error}");
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        // The name first: hosts see the item go before its objects do.
        if self.named {
            let _ = self.bus.release_name(self.name.as_str());
        }
        let objects = self.bus.object_server();
        if self.item {
            let _ = objects.remove::<Item, _>(ITEM_PATH);
        }
        if self.menu {
            let _ = objects.remove::<Menu, _>(MENU_PATH);
        }
    }
}

/// The item's status: a host may draw attention to an error or a question.
fn status(standing: Standing) -> &'static str {
    match standing {
        Standing::Running | Standing::Paused | Standing::Waiting => "Active",
        Standing::Stopped | Standing::Asks => "NeedsAttention",
    }
}

fn tip(presence: &Presence) -> (String, String) {
    (
        super::super::escape(&presence.title),
        super::super::escape(&presence.said),
    )
}

/// The icon at each size offered, badged.
fn pixmaps(icons: &mut Icons, badge: Badge) -> Vec<Pixmap> {
    SIZES
        .into_iter()
        .filter_map(|size| {
            let rgba = icons.draw(u32::from(size), badge)?;
            let argb = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [a, r, g, b])
                .collect();
            Some((i32::from(size), i32::from(size), argb))
        })
        .collect()
}

struct Item {
    look: Arc<Mutex<Look>>,
    asked: OnAsked,
}

impl Item {
    /// The window wanted, with the token the host gave just before.
    fn open(&self) {
        let token = self.look.lock().token.take();
        (self.asked)(Asked::Open { token });
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
// The bus's macro wants every method and property on `&self`, constant
// or not, and hands its arguments by value.
#[allow(clippy::unused_self, clippy::needless_pass_by_value)]
impl Item {
    /// The menu is shown by the host, from `Menu`.
    fn context_menu(&self, x: i32, y: i32) {
        let _ = (x, y);
    }

    fn activate(&self, x: i32, y: i32) {
        let _ = (x, y);
        self.open();
    }

    fn secondary_activate(&self, x: i32, y: i32) {
        let _ = (x, y);
        self.open();
    }

    fn scroll(&self, delta: i32, orientation: String) {
        let _ = (delta, orientation);
    }

    /// KDE's extension: the token for the click that follows (Wayland's
    /// xdg-activation).
    fn provide_xdg_activation_token(&self, token: String) {
        self.look.lock().token = Some(token);
    }

    #[zbus(property)]
    fn category(&self) -> &'static str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.look.lock().id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.look.lock().title.clone()
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        self.look.lock().status
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> &'static str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        self.look.lock().pixmaps.clone()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &'static str {
        ""
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> &'static str {
        ""
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &'static str {
        ""
    }

    /// The same icon: its badge already says what needs attention.
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<Pixmap> {
        self.look.lock().pixmaps.clone()
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> &'static str {
        ""
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        let look = self.look.lock();
        (
            String::new(),
            Vec::new(),
            look.tip.0.clone(),
            look.tip.1.clone(),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("a valid path")
    }

    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_attention_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_overlay_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

/// A menu entry as `GetLayout` lays it out: its id, its properties, its
/// entries (each a variant holding another).
#[derive(Debug, serde::Serialize, zbus::zvariant::Type)]
struct Node(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// The properties of the entry `id`, the pausing one by `pause`; `None`
/// for an id the menu does not have.
fn properties(id: i32, pause: Option<bool>) -> Option<Vec<(&'static str, Value<'static>)>> {
    Some(match id {
        0 => vec![("children-display", Value::from("submenu"))],
        OPEN => vec![("label", Value::from("Open Skwir"))],
        SEPARATOR => vec![("type", Value::from("separator"))],
        PAUSE => vec![
            (
                "label",
                Value::from(if pause == Some(false) {
                    "Go on"
                } else {
                    "Pause all"
                }),
            ),
            ("visible", Value::Bool(pause.is_some())),
        ],
        STOP => vec![("label", Value::from("Stop all and quit"))],
        _ => return None,
    })
}

/// The properties `names` asks of the entry `id`: all when none is named.
fn chosen(id: i32, pause: Option<bool>, names: &[String]) -> Option<HashMap<String, OwnedValue>> {
    let properties = properties(id, pause)?;
    Some(
        properties
            .into_iter()
            .filter(|(name, _)| names.is_empty() || names.iter().any(|named| named == name))
            .filter_map(|(name, value)| Some((name.to_owned(), OwnedValue::try_from(value).ok()?)))
            .collect(),
    )
}

struct Menu {
    look: Arc<Mutex<Look>>,
    asked: OnAsked,
}

impl Menu {
    /// An entry clicked: what it asks told, by what it says now.
    fn clicked(&self, id: i32) {
        let what = {
            let mut look = self.look.lock();
            match id {
                OPEN => Some(Asked::Open {
                    token: look.token.take(),
                }),
                PAUSE => look
                    .pause
                    .map(|pause| if pause { Asked::PauseAll } else { Asked::GoOn }),
                STOP => Some(Asked::StopAll),
                _ => None,
            }
        };
        if let Some(what) = what {
            (self.asked)(what);
        }
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
// The bus's macro wants every method and property on `&self`, constant
// or not, and hands its arguments by value.
#[allow(clippy::unused_self, clippy::needless_pass_by_value)]
impl Menu {
    /// The entry `parent_id` and, unless `recursion_depth` is 0, its
    /// entries: the menu is one level deep.
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Node)> {
        let (revision, pause) = {
            let look = self.look.lock();
            (look.revision, look.pause)
        };
        let own = chosen(parent_id, pause, &property_names)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no entry {parent_id}")))?;
        let entries = if parent_id == 0 && recursion_depth != 0 {
            ENTRIES
                .into_iter()
                .filter_map(|id| {
                    let properties = chosen(id, pause, &property_names)?;
                    let entry = StructureBuilder::new()
                        .add_field(id)
                        .add_field(properties)
                        .add_field(Vec::<OwnedValue>::new())
                        .build()
                        .ok()?;
                    OwnedValue::try_from(Value::from(entry)).ok()
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok((revision, Node(parent_id, own, entries)))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let pause = self.look.lock().pause;
        let ids = if ids.is_empty() {
            [0].into_iter().chain(ENTRIES).collect()
        } else {
            ids
        };
        ids.into_iter()
            .filter_map(|id| Some((id, chosen(id, pause, &property_names)?)))
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let pause = self.look.lock().pause;
        chosen(id, pause, std::slice::from_ref(&name))
            .and_then(|mut properties| properties.remove(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("entry {id} has no {name}")))
    }

    fn event(&self, id: i32, event_id: String, data: OwnedValue, timestamp: u32) {
        let _ = (data, timestamp);
        if event_id == "clicked" {
            self.clicked(id);
        }
    }

    /// Each event carried out; the ids the menu does not have answered.
    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        let mut unknown = Vec::new();
        for (id, event_id, data, timestamp) in events {
            if properties(id, None).is_none() {
                unknown.push(id);
            } else {
                self.event(id, event_id, data, timestamp);
            }
        }
        unknown
    }

    /// Nothing to make before it shows: no update needed.
    fn about_to_show(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        let unknown = ids
            .into_iter()
            .filter(|id| properties(*id, None).is_none())
            .collect();
        (Vec::new(), unknown)
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &'static str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}
