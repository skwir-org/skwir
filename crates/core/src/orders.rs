//! The orders places remember (ADR-132): a folder, or any place but a
//! search, the user asked to remember how it is sorted and grouped is
//! shown so wherever it is shown, the pane's own order set aside while it
//! is. What the user makes in skwir, kept in the data file (ADR-057)
//! under [`ORDERS`], one record a place.

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
}

/// The orders places remember, as the data file holds them: found by
/// place at once, and the folders at or below one in the time of what is
/// there, since a folder moves with what it holds.
#[derive(Clone, Debug, Default)]
pub struct Orders {
    /// By place, in the order they were remembered.
    by_place: IndexMap<Location, Order>,
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
                    orders.put(read.at, order);
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
            .filter_map(|(at, order)| {
                toml::Value::try_from(Record {
                    at: at.clone(),
                    key: order.key,
                    ascending: order.ascending,
                    group: order.group,
                    group_ascending: order.group_ascending,
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
        self.by_place.get(at).copied()
    }

    /// Remember `order` for `at`, or forget its own: whether that changed
    /// anything.
    pub fn set(&mut self, at: &Location, order: Option<Order>) -> bool {
        match order {
            Some(order) if self.of(at) == Some(order) => false,
            Some(order) => {
                self.put(at.clone(), order);
                true
            }
            None => self.take(at).is_some(),
        }
    }

    /// Follow what a plan changed, an undo's or a redo's too: a folder
    /// renamed or moved takes along what it and the folders it holds
    /// remember, two names swapped exchange theirs, and what is deleted
    /// for good forgets them. A folder trashed keeps them, for the restore
    /// that brings it back where it was. Whether any changed.
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
                _ => false,
            };
        }
        any
    }

    /// The folder at `from` is now at `to`: it, and every folder it holds,
    /// remembers its order there. Whether any did.
    pub fn moved(&mut self, from: &Path, to: &Path) -> bool {
        let taken = self.taken_below(from);
        let any = !taken.is_empty();
        for (path, order) in taken {
            if let Some(path) = moved_path(&path, from, to) {
                self.put(Location::from_path(path), order);
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
        for (path, order) in at_a {
            if let Some(path) = moved_path(&path, a, b) {
                self.put(Location::from_path(path), order);
            }
        }
        for (path, order) in at_b {
            if let Some(path) = moved_path(&path, b, a) {
                self.put(Location::from_path(path), order);
            }
        }
        any
    }

    /// Take out what `folder` and the folders below it remember, each
    /// with its path: in the time of what is there, found from it.
    fn taken_below(&mut self, folder: &Path) -> Vec<(PathBuf, Order)> {
        let below: Vec<PathBuf> = self
            .paths
            .range::<Path, _>((Bound::Included(folder), Bound::Unbounded))
            .take_while(|(path, _)| path.starts_with(folder))
            .map(|(path, _)| path.clone())
            .collect();
        below
            .into_iter()
            .filter_map(|path| {
                let at = self.paths.remove(&path)?;
                let order = self.by_place.shift_remove(&at)?;
                Some((path, order))
            })
            .collect()
    }

    /// Remember `order` at `at`, in place of what it remembered.
    fn put(&mut self, at: Location, order: Order) {
        if let Some(path) = at.path() {
            self.paths.insert(path, at.clone());
        }
        self.by_place.insert(at, order);
    }

    fn take(&mut self, at: &Location) -> Option<Order> {
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
    let weak = app.weak();
    app.executor.observe(
        "core",
        Arc::new(move |changes| {
            let Some(app) = weak.upgrade() else { return };
            let followed = app.workspace.write().follow_orders(changes);
            if followed {
                save(&app);
            }
        }),
    );
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
        assert!(!orders.follow(&[Change::Trash {
            path: "/files/b".into()
        }]));
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
