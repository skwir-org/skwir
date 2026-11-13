//! The orders places remember (ADR-132): a folder, or any place but a
//! search, the user asked to remember how it is sorted and grouped is
//! shown so wherever it is shown, the pane's own order set aside while it
//! is, and in the view it was shown in where that is one a folder may
//! remember (the list, the grid). What the user makes in skwir, kept in
//! the data file (ADR-057) under [`ORDERS`], one record a place.

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::app::{App, NotifyLevel};
use crate::grouping::GroupBy;
use crate::location::{Location, SortKey, SortSpec};
use crate::op::Change;
use crate::scheduler::Priority;
use crate::settings::SettingSpec;

/// The data setting of the orders places remember: a list of records,
/// each a place (`at`) and its order.
pub const ORDERS: &str = "core.orders";

/// What a place remembers of how it is shown: the sort and the groups,
/// each with its direction. Hidden files stay the pane's, and folders
/// first the user's, in every pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub key: SortKey,
    pub ascending: bool,
    #[serde(default)]
    pub group: GroupBy,
    #[serde(default = "going_up")]
    pub group_ascending: bool,
}

fn going_up() -> bool {
    true
}

impl Order {
    /// The order `sort` shows.
    pub fn of(sort: SortSpec) -> Self {
        Self {
            key: sort.key,
            ascending: sort.ascending,
            group: sort.group,
            group_ascending: sort.group_ascending,
        }
    }

    /// `sort` in this order, what is not of it (hidden files, folders
    /// first) left as it was.
    #[must_use]
    pub fn applied(self, sort: SortSpec) -> SortSpec {
        SortSpec {
            key: self.key,
            ascending: self.ascending,
            group: self.group,
            group_ascending: self.group_ascending,
            ..sort
        }
    }
}

/// What a place remembers: its order, and the view it is shown in, a
/// view's id the shell knows (a view a folder may remember,
/// `ViewSpec::remembered`); none while it was remembered in a view that
/// is the pane's (the columns, the tree).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remembered {
    pub order: Order,
    pub view: Option<Arc<str>>,
    /// Skwir put the folder in the trash (or one holding it): what it
    /// remembers waits for the restore that brings it back, and goes once
    /// no item of the trash came from there and no folder is there
    /// (`forget_emptied`).
    pub trashed: bool,
}

/// One record as the data file keeps it.
#[derive(Serialize, Deserialize)]
struct Record {
    at: Location,
    key: SortKey,
    ascending: bool,
    #[serde(default)]
    group: GroupBy,
    #[serde(default = "going_up")]
    group_ascending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    trashed: bool,
}

/// The orders places remember, as the data file holds them: found by
/// place at once, and the folders at or below one in the time of what is
/// there, since a folder moves with what it holds.
#[derive(Clone, Debug, Default)]
pub struct Orders {
    /// By place, in the order they were remembered.
    by_place: IndexMap<Location, Remembered>,
    /// The folders among them by path, each with its place as kept. Path
    /// order compares components, so the folders at or below one are one
    /// run of it, found from it.
    paths: BTreeMap<PathBuf, Location>,
    /// Records that do not read (a later skwir's criterion, a hand edit),
    /// as written: kept when the list is written again.
    unread: Vec<toml::Value>,
    /// The setting holds no list: nothing is written over it.
    unreadable: bool,
}

impl Orders {
    /// The orders `value` (the data setting's) holds, read one record at a
    /// time: one that does not read is left out and kept as written.
    pub fn read(value: Option<toml::Value>) -> Self {
        let mut orders = Self::default();
        let records = match value {
            None => return orders,
            Some(toml::Value::Array(records)) => records,
            Some(_) => {
                tracing::warn!("{ORDERS} is not a list: no place's order is shown");
                orders.unreadable = true;
                return orders;
            }
        };
        for (at, record) in records.into_iter().enumerate() {
            match record.clone().try_into::<Record>() {
                Ok(read) => {
                    let order = Order {
                        key: read.key,
                        ascending: read.ascending,
                        group: read.group,
                        group_ascending: read.group_ascending,
                    };
                    let view = read.view.filter(|view| !view.is_empty()).map(Arc::from);
                    orders.put(
                        read.at,
                        Remembered {
                            order,
                            view,
                            trashed: read.trashed,
                        },
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        record = at,
                        %error,
                        "a place's order does not read: left out, kept as written"
                    );
                    orders.unread.push(record);
                }
            }
        }
        orders
    }

    /// The list to write, those that did not read after the others; none
    /// where the setting held no list, which is not written over.
    pub fn to_value(&self) -> Option<toml::Value> {
        if self.unreadable {
            return None;
        }
        let records = self
            .by_place
            .iter()
            .filter_map(|(at, kept)| {
                let order = kept.order;
                toml::Value::try_from(Record {
                    at: at.clone(),
                    key: order.key,
                    ascending: order.ascending,
                    group: order.group,
                    group_ascending: order.group_ascending,
                    view: kept.view.as_deref().map(str::to_owned),
                    trashed: kept.trashed,
                })
                .map_err(|error| tracing::warn!(%at, %error, "a place's order is not written"))
                .ok()
            })
            .chain(self.unread.iter().cloned())
            .collect();
        Some(toml::Value::Array(records))
    }

    /// Whether the setting holds no list, so nothing is remembered.
    pub fn is_unreadable(&self) -> bool {
        self.unreadable
    }

    pub fn is_empty(&self) -> bool {
        self.by_place.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_place.len()
    }

    /// The order `at` remembers, if it remembers one.
    pub fn of(&self, at: &Location) -> Option<Order> {
        self.by_place.get(at).map(|kept| kept.order)
    }

    /// The view `at` remembers, if it remembers one.
    pub fn view_of(&self, at: &Location) -> Option<Arc<str>> {
        self.by_place.get(at).and_then(|kept| kept.view.clone())
    }

    /// Remember `order` for `at`, the view it remembers kept, or forget
    /// all it remembers: whether that changed anything.
    pub fn set(&mut self, at: &Location, order: Option<Order>) -> bool {
        match (order, self.by_place.get_mut(at)) {
            (Some(order), Some(kept)) if kept.order == order => false,
            (Some(order), Some(kept)) => {
                kept.order = order;
                true
            }
            (Some(order), None) => {
                self.put(
                    at.clone(),
                    Remembered {
                        order,
                        view: None,
                        trashed: false,
                    },
                );
                true
            }
            (None, _) => self.take(at).is_some(),
        }
    }

    /// Remember `view` as the one `at` is shown in, or none, where `at`
    /// remembers its order: whether that changed anything.
    pub fn set_view(&mut self, at: &Location, view: Option<Arc<str>>) -> bool {
        match self.by_place.get_mut(at) {
            Some(kept) if kept.view != view => {
                kept.view = view;
                true
            }
            _ => false,
        }
    }

    /// Follow what a plan changed, an undo's or a redo's too: a folder
    /// renamed or moved takes along what it and the folders it holds
    /// remember, two names swapped exchange theirs, and what is deleted
    /// for good forgets them. A folder trashed keeps them, said trashed,
    /// for the restore that brings it back where it was, which says them
    /// shown again; once the trash no longer holds it, they go
    /// (`forget_emptied`). Whether any changed.
    pub fn follow(&mut self, changes: &[Change]) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let mut any = false;
        for change in changes {
            any |= match change {
                Change::Rename { from, to } | Change::Move { from, to } => self.moved(from, to),
                Change::Swap { a, b } => self.swapped(a, b),
                Change::Delete { path } | Change::Remove { path } => {
                    !self.taken_below(path).is_empty()
                }
                Change::Trash { path } => self.said_trashed(path, true),
                Change::Restore { path } => self.said_trashed(path, false),
                _ => false,
            };
        }
        any
    }

    /// Say what `folder` and the folders below it remember trashed, or
    /// not: whether any changed.
    fn said_trashed(&mut self, folder: &Path, trashed: bool) -> bool {
        let mut any = false;
        for at in self.below(folder) {
            if let Some(kept) = self.by_place.get_mut(&at)
                && kept.trashed != trashed
            {
                kept.trashed = trashed;
                any = true;
            }
        }
        any
    }

    /// The folders remembered as trashed (`Remembered::trashed`).
    pub fn trashed(&self) -> Vec<PathBuf> {
        self.paths
            .iter()
            .filter(|(_, at)| self.by_place.get(*at).is_some_and(|kept| kept.trashed))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Take in what a look at the trash found (`emptied`, on a worker): of
    /// the folders remembered trashed, those back where they were are no
    /// longer trashed, and those that are nowhere, neither there nor in
    /// the trash, forget what they remember. Whether any changed.
    pub fn forget_emptied(&mut self, found: &Emptied) -> bool {
        let mut any = false;
        for path in &found.back {
            any |= self.said_trashed(path, false);
        }
        for path in &found.gone {
            if let Some(at) = self.paths.get(path).cloned()
                && self.by_place.get(&at).is_some_and(|kept| kept.trashed)
            {
                self.take(&at);
                any = true;
            }
        }
        any
    }

    /// The folder at `from` is now at `to`: it, and every folder it holds,
    /// remembers its order there. Whether any did.
    pub fn moved(&mut self, from: &Path, to: &Path) -> bool {
        let taken = self.taken_below(from);
        let any = !taken.is_empty();
        for (path, kept) in taken {
            if let Some(path) = moved_path(&path, from, to) {
                self.put(Location::from_path(path), kept);
            }
        }
        any
    }

    /// The folders at `a` and `b` exchanged names: each, with what it
    /// holds, remembers its order at the other's. Whether any did.
    fn swapped(&mut self, a: &Path, b: &Path) -> bool {
        let at_a = self.taken_below(a);
        let at_b = self.taken_below(b);
        let any = !at_a.is_empty() || !at_b.is_empty();
        for (path, kept) in at_a {
            if let Some(path) = moved_path(&path, a, b) {
                self.put(Location::from_path(path), kept);
            }
        }
        for (path, kept) in at_b {
            if let Some(path) = moved_path(&path, b, a) {
                self.put(Location::from_path(path), kept);
            }
        }
        any
    }

    /// The places of `folder` and of the folders below it that remember:
    /// in the time of what is there, found from it.
    fn below(&self, folder: &Path) -> Vec<Location> {
        self.paths
            .range::<Path, _>((Bound::Included(folder), Bound::Unbounded))
            .take_while(|(path, _)| path.starts_with(folder))
            .map(|(_, at)| at.clone())
            .collect()
    }

    /// Take out what `folder` and the folders below it remember, each
    /// with its path: in the time of what is there, found from it.
    fn taken_below(&mut self, folder: &Path) -> Vec<(PathBuf, Remembered)> {
        self.below(folder)
            .into_iter()
            .filter_map(|at| {
                let path = at.path()?;
                self.paths.remove(&path)?;
                let kept = self.by_place.shift_remove(&at)?;
                Some((path, kept))
            })
            .collect()
    }

    /// Remember `kept` at `at`, in place of what it remembered.
    fn put(&mut self, at: Location, kept: Remembered) {
        if let Some(path) = at.path() {
            self.paths.insert(path, at.clone());
        }
        self.by_place.insert(at, kept);
    }

    fn take(&mut self, at: &Location) -> Option<Remembered> {
        if let Some(path) = at.path() {
            self.paths.remove(&path);
        }
        self.by_place.shift_remove(at)
    }
}

/// Declare what places remember and take it in; from now on a plan that
/// renames, moves, swaps or deletes a folder (an undo's and a redo's too)
/// takes along, or forgets, what it and the folders it holds remember.
pub(crate) fn register(app: &App) {
    app.settings.declare(SettingSpec::data(
        ORDERS,
        "The orders places remember (ADR-132), each { at, key, ascending, group, group_ascending }: shown so in every pane",
        toml::Value::Array(Vec::new()),
    ));
    apply(app);
    look_at_trash(app);
    let weak = app.weak();
    app.executor.observe(
        "core",
        Arc::new(move |changes| {
            let Some(app) = weak.upgrade() else { return };
            let followed = app.workspace.write().follow_orders(changes);
            if followed {
                save(&app);
            }
            // Something deleted for good, the trash emptied among others:
            // what the folders trashed remember may be for none now.
            if changes
                .iter()
                .any(|c| matches!(c, Change::Delete { .. } | Change::Remove { .. }))
            {
                look_at_trash(&app);
            }
        }),
    );
}

/// What a look at the trash found of the folders remembered trashed
/// (`emptied`).
#[derive(Debug, Default)]
pub struct Emptied {
    /// Back where they were, and no item of the trash from there.
    pub back: Vec<PathBuf>,
    /// Neither there nor in the trash.
    pub gone: Vec<PathBuf>,
}

/// Look, on a worker, at whether the trash still holds the folders
/// remembered trashed (`Remembered::trashed`), and forget those it no
/// longer holds and that are not back (`Orders::forget_emptied`). Asked at
/// the start (the trash may have been emptied since) and once a plan
/// deleted something for good.
fn look_at_trash(app: &App) {
    let trashed = app.workspace.read().orders().trashed();
    if trashed.is_empty() {
        return;
    }
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Background,
        "orders.trash",
        app.lifetime().child(),
        move || {
            let found = emptied(trashed);
            if found.back.is_empty() && found.gone.is_empty() {
                return;
            }
            let Some(app) = weak.upgrade() else { return };
            if app.workspace.write().forget_emptied(&found) {
                save(&app);
            }
        },
    );
}

/// Of `trashed`, the folders back where they were and those gone, neither
/// there nor in the trash: on this machine's own volumes, whose trashes
/// are read (`op::trash_items_here`); a folder of a remote's, whose trash
/// is not, is never said gone. Reads every record of the trash: on a
/// worker.
#[cfg(target_os = "linux")]
pub fn emptied(trashed: Vec<PathBuf>) -> Emptied {
    let from: std::collections::HashSet<PathBuf> = crate::op::trash_items_here()
        .iter()
        .map(trash::TrashItem::original_path)
        .collect();
    let mut found = Emptied::default();
    for path in trashed {
        if crate::fs::reach::of(&path).is_some()
            || path.ancestors().any(|above| from.contains(above))
        {
            continue;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => found.back.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => found.gone.push(path),
            _ => {}
        }
    }
    found
}

/// Elsewhere the trash is not read here: a folder remembered trashed
/// keeps what it remembers until it is deleted for good.
#[cfg(not(target_os = "linux"))]
pub fn emptied(_trashed: Vec<PathBuf>) -> Emptied {
    Emptied::default()
}

/// Take in what the data file says places remember: at the start, and
/// when another skwir (or a hand edit) changed it. Each pane shows its
/// place's order.
pub(crate) fn apply(app: &App) {
    let orders = Orders::read(app.settings.get::<toml::Value>(ORDERS));
    app.workspace.write().set_orders(orders);
}

/// Write down what places remember, in the data file: on a worker where a
/// window draws, here otherwise (a headless caller may end once answered).
/// A file that cannot take it is said.
pub(crate) fn save(app: &App) {
    {
        let ws = app.workspace.read();
        let Some(value) = ws.orders().to_value() else {
            return;
        };
        // Set under the workspace's lock, so changes made at once are
        // set in the order they were made.
        app.settings.set(ORDERS, value);
    }
    let weak = app.weak();
    let write = move || {
        let Some(app) = weak.upgrade() else { return };
        if let Err(error) = app.settings.save() {
            app.notify(
                NotifyLevel::Error,
                format!("The order was not remembered: {error:#}"),
            );
        }
    };
    if crate::scheduler::has_window() {
        app.scheduler.spawn(
            Priority::Normal,
            "orders.save",
            app.lifetime().child(),
            write,
        );
    } else {
        write();
    }
}

/// Where `path` is once the folder at `from` is at `to`: none when it is
/// not at or below `from`.
fn moved_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_date() -> Order {
        Order {
            key: SortKey::Modified,
            ascending: false,
            group: GroupBy::Key(SortKey::Kind),
            group_ascending: true,
        }
    }

    fn at(path: &str) -> Location {
        Location::from_path(path)
    }

    #[test]
    fn an_order_is_the_sort_and_the_groups_hidden_files_and_folders_first_left_as_they_are() {
        let pane = SortSpec {
            show_hidden: true,
            folders_first: false,
            ..SortSpec::default()
        };
        let shown = by_date().applied(pane);
        assert_eq!(Order::of(shown), by_date());
        assert!(shown.show_hidden);
        assert!(!shown.folders_first);
    }

    #[test]
    fn what_places_remember_is_written_and_read_again_those_that_do_not_read_kept() {
        let mut orders = Orders::default();
        assert!(orders.set(&at("/files/downloads"), Some(by_date())));
        assert!(
            !orders.set(&at("/files/downloads"), Some(by_date())),
            "the same again changes nothing"
        );
        assert!(orders.set(&Location::new("trash", ""), Some(by_date())));
        let mut written = orders.to_value().unwrap();
        // A later skwir's criterion, which this one cannot read.
        let later: toml::Value = toml::from_str::<toml::Table>(
            "at = { scheme = \"path\", rest = \"/later\" }\nkey = \"loudness\"\nascending = true",
        )
        .unwrap()
        .into();
        written.as_array_mut().unwrap().push(later.clone());
        let read = Orders::read(Some(written));
        assert_eq!(read.len(), 2);
        assert_eq!(read.of(&at("/files/downloads")), Some(by_date()));
        assert_eq!(read.of(&Location::new("trash", "")), Some(by_date()));
        assert_eq!(read.of(&at("/files")), None);
        let again = read.to_value().unwrap();
        assert_eq!(
            again.as_array().unwrap().last(),
            Some(&later),
            "kept as written"
        );
    }

    #[test]
    fn a_place_remembers_a_view_with_its_order_and_takes_it_along() {
        let mut orders = Orders::default();
        assert!(
            !orders.set_view(&at("/files/a"), Some("grid".into())),
            "no view without an order"
        );
        orders.set(&at("/files/a"), Some(by_date()));
        assert!(orders.set_view(&at("/files/a"), Some("grid".into())));
        assert!(!orders.set_view(&at("/files/a"), Some("grid".into())));
        // A new order keeps the view.
        let by_name = Order {
            key: SortKey::Name,
            ..by_date()
        };
        assert!(orders.set(&at("/files/a"), Some(by_name)));
        assert_eq!(orders.view_of(&at("/files/a")).as_deref(), Some("grid"));
        let read = Orders::read(orders.to_value());
        assert_eq!(read.view_of(&at("/files/a")).as_deref(), Some("grid"));
        assert_eq!(read.of(&at("/files/a")), Some(by_name));
        let mut read = read;
        read.moved(Path::new("/files/a"), Path::new("/files/b"));
        assert_eq!(read.view_of(&at("/files/b")).as_deref(), Some("grid"));
        assert!(read.set(&at("/files/b"), None));
        assert_eq!(read.view_of(&at("/files/b")), None);
    }

    /// A folder trashed keeps what it and the folders it holds remember,
    /// said trashed, written so, until a restore brings it back or the
    /// trash no longer holds it: then forgotten, but never a folder that
    /// was not trashed, nor one back where it was.
    #[test]
    fn a_folder_trashed_forgets_once_the_trash_no_longer_holds_it() {
        let mut orders = Orders::default();
        for path in ["/files/a", "/files/a/b", "/files/c", "/files/d"] {
            orders.set(&at(path), Some(by_date()));
        }
        assert!(orders.follow(&[
            Change::Trash {
                path: "/files/a".into()
            },
            Change::Trash {
                path: "/files/d".into()
            },
        ]));
        assert_eq!(
            orders.trashed(),
            [
                PathBuf::from("/files/a"),
                PathBuf::from("/files/a/b"),
                PathBuf::from("/files/d")
            ]
        );
        let read = Orders::read(orders.to_value());
        assert_eq!(read.trashed(), orders.trashed(), "written trashed");
        assert!(orders.follow(&[Change::Restore {
            path: "/files/d".into()
        }]));
        assert_eq!(orders.trashed().len(), 2, "restored: shown again");
        // The trash emptied: the trashed folder is nowhere; one that was
        // never trashed is not forgotten for being gone.
        let found = Emptied {
            back: Vec::new(),
            gone: vec!["/files/a".into(), "/files/a/b".into(), "/files/c".into()],
        };
        assert!(orders.forget_emptied(&found));
        assert_eq!(orders.of(&at("/files/a")), None);
        assert_eq!(orders.of(&at("/files/a/b")), None);
        assert_eq!(orders.of(&at("/files/c")), Some(by_date()));
        assert_eq!(orders.of(&at("/files/d")), Some(by_date()));
        assert!(!orders.forget_emptied(&found), "once");
    }

    /// The trash looked at: a folder remembered trashed that is back is
    /// no longer trashed, one neither there nor in the trash is gone.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_look_at_the_trash_tells_back_from_gone() {
        let dir = tempfile::tempdir().unwrap();
        let back = dir.path().join("back");
        std::fs::create_dir(&back).unwrap();
        let gone = dir.path().join("gone");
        let found = emptied(vec![back.clone(), gone.clone()]);
        assert_eq!(found.back, [back]);
        assert_eq!(found.gone, [gone]);
    }

    #[test]
    fn a_setting_that_holds_no_list_is_never_written_over() {
        let orders = Orders::read(Some(toml::Value::String("by date".into())));
        assert!(orders.is_unreadable());
        assert!(orders.is_empty());
        assert_eq!(orders.to_value(), None);
    }

    #[test]
    fn a_folder_moved_takes_along_what_it_and_the_folders_it_holds_remember() {
        let mut orders = Orders::default();
        for path in [
            "/files/a",
            "/files/a/b",
            "/files/a b",
            "/files/ab",
            "/other",
        ] {
            orders.set(&at(path), Some(by_date()));
        }
        assert!(orders.follow(&[Change::Rename {
            from: "/files/a".into(),
            to: "/files/c".into(),
        }]));
        for path in [
            "/files/c",
            "/files/c/b",
            "/files/a b",
            "/files/ab",
            "/other",
        ] {
            assert_eq!(orders.of(&at(path)), Some(by_date()), "{path}");
        }
        assert_eq!(orders.of(&at("/files/a")), None);
        assert_eq!(orders.of(&at("/files/a/b")), None);
        assert!(!orders.follow(&[Change::Move {
            from: "/files/nothing".into(),
            to: "/files/c/nothing".into(),
        }]));
        // Undone, it goes back.
        assert!(orders.follow(&[Change::Move {
            from: "/files/c".into(),
            to: "/files/a".into(),
        }]));
        assert_eq!(orders.of(&at("/files/a/b")), Some(by_date()));
        assert_eq!(orders.len(), 5);
    }

    #[test]
    fn names_swapped_exchange_what_they_remember_and_what_is_deleted_forgets() {
        let by_name = Order {
            key: SortKey::Name,
            ascending: true,
            group: GroupBy::None,
            group_ascending: true,
        };
        let mut orders = Orders::default();
        orders.set(&at("/files/a"), Some(by_date()));
        orders.set(&at("/files/b/c"), Some(by_name));
        assert!(orders.follow(&[Change::Swap {
            a: "/files/a".into(),
            b: "/files/b".into(),
        }]));
        assert_eq!(orders.of(&at("/files/b")), Some(by_date()));
        assert_eq!(orders.of(&at("/files/a/c")), Some(by_name));
        assert!(
            orders.follow(&[Change::Trash {
                path: "/files/b".into()
            }]),
            "said trashed"
        );
        assert_eq!(
            orders.of(&at("/files/b")),
            Some(by_date()),
            "kept for the restore"
        );
        assert!(orders.follow(&[Change::Delete {
            path: "/files/a".into()
        }]));
        assert_eq!(orders.of(&at("/files/a/c")), None);
        assert!(orders.set(&at("/files/b"), None));
        assert!(orders.is_empty());
    }
}
