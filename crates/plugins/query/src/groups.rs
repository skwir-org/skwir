//! The query plugin's sorter (ADR-130): what a sort or a grouping needs of
//! an entry that core cannot read, a kind, a type's name, an owner's name,
//! and every criterion's groups, their words and the query term that keeps
//! each one's files.
//!
//! Each group is made once per keys and shared by every entry in it. Keys
//! answer from memory: a type's name not read yet is read on a worker,
//! what the Type column calls its files standing for it meanwhile
//! (`mime::unnamed`), and the keys say they changed once it is
//! (`Keys::changed`); so are the users' names, an owner's number standing
//! for its name.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock, Weak};
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, Value, keys};
use skwir_core::grouping::{Group, Keyer, Keys, Of};
use skwir_core::location::{SortKey, extension_of, tags_of};
use skwir_core::scheduler::Priority;
use skwir_core::words;
use skwir_core::words::{Moment, Shape};
use skwir_platform::calendar::{Calendar, Period, date_string};
use skwir_platform::mime::{self, Kind, Mime};

/// Keys for a sort by `sort` grouped by `group`, with the user's calendar
/// and the time now: what the sorter answers (`registry::Sorter`).
pub fn keys(app: &Weak<App>, sort: SortKey, group: Option<SortKey>) -> Option<Keys> {
    keys_at(app, sort, group, &Calendar::system(), SystemTime::now())
}

/// [`keys`] for `cal`, at `now`.
pub fn keys_at(
    app: &Weak<App>,
    sort: SortKey,
    group: Option<SortKey>,
    cal: &Calendar,
    now: SystemTime,
) -> Option<Keys> {
    let mut keys = Keys::new(sort, group);
    // A kind and a type are read from the name in the type database.
    keys.costly = [Some(sort), group]
        .into_iter()
        .flatten()
        .any(|key| matches!(key, SortKey::Kind | SortKey::Type));
    if sort.keyed() {
        keys.value = Some(keyer(app, &keys, sort, cal, now).0);
    }
    if let Some(group) = group {
        let (keyer, next_change) = keyer(app, &keys, group, cal, now);
        keys.group = Some(keyer);
        keys.next_change = next_change;
    }
    (keys.value.is_some() || keys.group.is_some()).then_some(keys)
}

/// What gives each entry its group by `key`, and when the groups next
/// change.
fn keyer(
    app: &Weak<App>,
    keys: &Keys,
    key: SortKey,
    cal: &Calendar,
    now: SystemTime,
) -> (Keyer, Option<SystemTime>) {
    match key {
        SortKey::Name => (by_name(), None),
        SortKey::Kind => (by_kind(), None),
        SortKey::Type => (by_type(app, keys), None),
        SortKey::Extension => (by_extension(), None),
        SortKey::Size => (by_size(), None),
        SortKey::Modified
        | SortKey::Created
        | SortKey::Accessed
        | SortKey::Deleted
        | SortKey::Taken => {
            let dates = Dates::at(key, cal, now);
            let next = cal.day_start(dates.today + 1);
            let cal = cal.clone();
            (Arc::new(move |e: &Of<'_>| dates.of(e, &cal)), Some(next))
        }
        SortKey::Rating => (by_rating(), None),
        SortKey::Tags => (by_tags(), None),
        SortKey::Owner => (by_owner(app, keys), None),
        SortKey::Folder => (by_folder(), None),
        SortKey::Dimensions => (by_dimensions(), None),
        SortKey::Duration => (by_duration(), None),
        SortKey::Vcs => (by_vcs(), None),
    }
}

/// By the folder each is in, as the sort finds it (`Sort::folder_of`):
/// one group a folder, in the order of their paths, called by its path
/// from the home folder (`~/notes`); a place in none. No term keeps them:
/// a filter tests a file by itself, not where it is.
fn by_folder() -> Keyer {
    let made = Made::new();
    let none = Arc::new(Group::ranked(
        0,
        &words!(
            query_group_no_folder = "No folder",
            note = "A group's heading, grouped by folder: what is in no folder (a place)."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| match e.folder {
        Some(folder) => made.get(folder.to_path_buf(), || Group {
            rank: 1,
            key: Arc::from(skwir_core::collate::sort_key(folder.as_os_str())),
            label: Arc::from(skwir_core::exclusions::tilde(folder)),
            folders: false,
            term: None,
        }),
        None => Arc::clone(&none),
    })
}

/// The groups made so far, by what makes them: one each.
struct Made<K>(Mutex<HashMap<K, Arc<Group>>>);

impl<K: std::hash::Hash + Eq> Made<K> {
    fn new() -> Arc<Self> {
        Arc::new(Made(Mutex::new(HashMap::new())))
    }

    fn get(&self, key: K, make: impl FnOnce() -> Group) -> Arc<Group> {
        Arc::clone(self.0.lock().entry(key).or_insert_with(|| Arc::new(make())))
    }
}

/// By the heading the names' order puts each under (`collate::initial`,
/// CLDR's index for the locale: É under E; Ä under A in German, after Z
/// in Swedish; か for ガ in Japanese, ㄱ for 가 in Korean, pinyin's A for
/// 阿 in Chinese): Other for a name that begins with neither a letter nor
/// a digit, then 0–9, first; the letters no heading is for (a kanji) last.
/// Each group found by the name's beginning, as the collation reads it
/// once: nothing made per entry.
fn by_name() -> Keyer {
    by_name_in(skwir_core::collate::current())
}

/// [`by_name`] by `collation`'s headings.
fn by_name_in(collation: &'static skwir_core::collate::Collation) -> Keyer {
    use skwir_core::collate::Initial;
    let made = Made::<Initial>::new();
    Arc::new(move |e: &Of<'_>| {
        let initial = collation.initial(&e.name);
        match &initial {
            Initial::Digit => made.get(initial, || {
                Group::ranked(
                    1,
                    &words!(
                        query_group_digits = "0–9",
                        note = "A group's heading, grouped by name: names that begin with a digit."
                    )
                    .say(),
                )
            }),
            // Ordered among the letters by the heading's collation key.
            Initial::Letter(heading) => {
                let heading = Arc::clone(heading);
                made.get(initial, || Group::new(2, &heading))
            }
            // Where the order puts that count, among the letters.
            Initial::Strokes { strokes, key } => {
                let (strokes, key) = (*strokes, Arc::clone(key));
                made.get(initial, || Group {
                    rank: 2,
                    key,
                    label: Arc::from(
                        words!(
                            query_group_strokes = "{ $strokes ->
                                [one] { $strokes } stroke
                               *[other] { $strokes } strokes
                            }",
                            strokes = strokes,
                            note = "A group's heading, grouped by name in Traditional Chinese's order, which heads Han characters by how many strokes they are written in (1劃, 2劃…)."
                        )
                        .say(),
                    ),
                    folders: false,
                    term: None,
                })
            }
            Initial::Unheaded => made.get(initial, || {
                Group::ranked(
                    3,
                    &words!(
                        query_group_other_letters = "Other letters",
                        note = "A group's heading, grouped by name, after the letters' groups: names that begin with a letter no heading is for (a Japanese name's kanji, a Han character in an order other than Chinese's)."
                    )
                    .say(),
                )
            }),
            Initial::Other => made.get(initial, || {
                Group::ranked(
                    0,
                    &words!(
                        query_group_other_names = "Other",
                        note = "A group's heading, grouped by name: names that begin with neither a digit nor a letter."
                    )
                    .say(),
                )
            }),
        }
    })
}

/// What the type database calls an entry's kind: its first, A to Z by
/// what people call it; Other for a file the database puts in none.
fn by_kind() -> Keyer {
    let groups: HashMap<Kind, Arc<Group>> = Kind::ALL
        .into_iter()
        .map(|kind| {
            // Said as the groups are keyed: A to Z in the reader's words.
            let group =
                Group::new(1, &kind.words().say()).with_term(format!("kind:{}", kind.name()));
            let group = if kind == Kind::Folder {
                group.of_folders()
            } else {
                group
            };
            (kind, Arc::new(group))
        })
        .collect();
    let other = Arc::new(Group::ranked(
        0,
        &words!(
            query_group_other_kind = "Other",
            note = "A group's heading, grouped by kind: files of no kind (images, documents…)."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        let kind = if e.is_dir() {
            Some(Kind::Folder)
        } else {
            mime::of_entry(e).and_then(|m| mime::kinds(&m).into_iter().next())
        };
        kind.and_then(|k| groups.get(&k))
            .map_or_else(|| Arc::clone(&other), Arc::clone)
    })
}

/// The types whose names were read, by type, and the words' revision
/// they were read at: shared by every keys while that revision stands. A
/// type's name is the database's comment in the reader's languages
/// (`Mime::description`, `locale::preferred`), which a language chosen
/// changes, so it depends on the type and on `words::revision()` alone.
#[derive(Default)]
struct TypeGroups {
    said_at: u64,
    groups: HashMap<String, TypeName>,
}

/// What the database said of a type's name.
#[derive(Clone)]
enum TypeName {
    /// Its group, called by its name.
    Named(Arc<Group>),
    /// It names it not: its files are called by their names
    /// (`mime::unnamed`).
    Unnamed,
}

fn type_groups() -> &'static RwLock<TypeGroups> {
    static NAMES: OnceLock<RwLock<TypeGroups>> = OnceLock::new();
    NAMES.get_or_init(RwLock::default)
}

/// What the database said of type `mime`'s name, if it was read in the
/// words said now.
fn known_type_group(mime: &str) -> Option<TypeName> {
    let now = words::revision();
    let kept = type_groups().read();
    (kept.said_at == now)
        .then(|| kept.groups.get(mime).cloned())
        .flatten()
}

/// What the database says of type `mime`'s name, read now if it is not
/// known in the words said now. What was read before the words moved is
/// forgotten once they have, and a name read while they moved is not
/// kept.
fn type_group(mime: &Mime) -> TypeName {
    let now = words::revision();
    if let Some(group) = known_type_group(mime.as_str()) {
        return group;
    }
    let group = mime.description().map_or(TypeName::Unnamed, |name| {
        TypeName::Named(Arc::new(of_type(
            mime,
            Group::new(1, &name).with_term(format!("mime:{mime}")),
        )))
    });
    let mut kept = type_groups().write();
    // The revisions only go up: one kept from before is let go of, and
    // one read before the kept one's is said in words gone.
    if kept.said_at < now {
        *kept = TypeGroups {
            said_at: now,
            groups: HashMap::new(),
        };
    }
    if kept.said_at != now {
        return group;
    }
    kept.groups
        .entry(mime.as_str().to_owned())
        .or_insert(group)
        .clone()
}

/// `group`, of type `mime`: one of only folders where `mime` is a
/// folder's.
fn of_type(mime: &Mime, group: Group) -> Group {
    if mime.as_str() == "inode/directory" {
        group.of_folders()
    } else {
        group
    }
}

/// By what the type is called (`JPEG image`). Its name is read from the
/// database, which reads files: on a worker (the sort's), at once; on the
/// window's thread (what arrives, placed), what the Type column calls a
/// file meanwhile stands for it (`mime::unnamed`: `PNG file`) while a
/// worker reads it, and the keys say they changed when it has. A type the
/// database names not is called so for good, never by its id.
fn by_type(app: &Weak<App>, keys: &Keys) -> Keyer {
    let app = app.clone();
    let changed = Arc::clone(&keys.changed);
    let asked: Arc<Mutex<std::collections::HashSet<String>>> = Arc::default();
    // By the type and what the file is called by its name, each made once.
    let unnamed_made = Made::<(String, Option<String>)>::new();
    let unnamed = move |mime: &Mime, e: &Of<'_>| {
        let extension = mime::unnamed_extension(&e.type_name().to_string_lossy());
        let key = (mime.as_str().to_owned(), extension);
        let label = || mime::unnamed_by(key.1.as_deref()).say();
        let group = || {
            of_type(
                mime,
                Group::new(1, &label()).with_term(format!("mime:{mime}")),
            )
        };
        unnamed_made.get(key.clone(), group)
    };
    let unknown = Arc::new(Group::ranked(
        0,
        &words!(
            query_group_unknown_type = "Unknown type",
            note = "A group's heading, grouped by type: files whose type is not known."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        let Some(mime) = mime::of_entry(e) else {
            return Arc::clone(&unknown);
        };
        let named = |name: TypeName| match name {
            TypeName::Named(group) => group,
            TypeName::Unnamed => unnamed(&mime, e),
        };
        if let Some(name) = known_type_group(mime.as_str()) {
            return named(name);
        }
        if !skwir_core::scheduler::on_window_thread() {
            return named(type_group(&mime));
        }
        if asked.lock().insert(mime.as_str().to_owned())
            && let Some(app) = app.upgrade()
        {
            let (changed, woken) = (Arc::clone(&changed), app.weak());
            let wanted = mime.clone();
            app.scheduler.spawn(
                Priority::Visible,
                "query.type-name",
                CancelToken::new(),
                move || {
                    type_group(&wanted);
                    changed.fetch_add(1, Ordering::AcqRel);
                    if let Some(app) = woken.upgrade() {
                        app.wake();
                    }
                },
            );
        }
        unnamed(&mime, e)
    })
}

/// By extension (`.jpg`), case folded; No extension, and Folders, which
/// have none, first.
fn by_extension() -> Keyer {
    let made = Made::<String>::new();
    let folders = Arc::new(Group::ranked(0, &folders_heading()).of_folders());
    let none = Arc::new(Group::ranked(
        1,
        &words!(
            query_group_no_extension = "No extension",
            note = "A group's heading, grouped by extension."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        if e.is_dir() {
            return Arc::clone(&folders);
        }
        let Some(ext) = extension_of(e) else {
            return Arc::clone(&none);
        };
        let ext = ext.to_lowercase();
        made.get(ext.clone(), || {
            Group::new(2, &format!(".{ext}")).with_term(format!("ext:{ext}"))
        })
    })
}

/// The heading of the group of folders, where folders have none of what
/// groups (no extension, no size, no dimensions, no duration).
fn folders_heading() -> String {
    words!(
        query_group_folders = "Folders",
        note = "A group's heading: the folders, which have none of what the files are grouped by."
    )
    .say()
}

/// The size groups' upper bounds in bytes, in the units sizes are shown in
/// (`fs::size_units`, ADR-139, 9), and what they are called and kept by,
/// their terms in those units; Over 1 GB after, its lower bound last.
fn sizes() -> (Vec<(u64, String, String)>, (String, String)) {
    let units = skwir_core::fs::size_units();
    let [_, k, m, g, ..] = units.names();
    (
        vec![
            (
                1,
                words!(
                    query_group_empty = "Empty",
                    note = "A group's heading, grouped by size: empty files."
                )
                .say(),
                "size:0".to_owned(),
            ),
            (
                units.bytes(100, 1),
                words!(
                    query_group_under_100 = "Under 100\u{a0}{ $unit }",
                    unit = k,
                    note = "A group's heading, grouped by size. unit: a unit of size, as sizes are shown (kB, KiB)."
                )
                .say(),
                format!("size>0 size<100{k}"),
            ),
            (
                units.bytes(1, 2),
                words!(
                    query_group_100_to_1 = "100\u{a0}{ $unit } to 1\u{a0}{ $next }",
                    unit = k,
                    next = m,
                    note = "A group's heading, grouped by size. unit and next: units of size, as sizes are shown (kB and MB)."
                )
                .say(),
                format!("size>=100{k} size<1{m}"),
            ),
            (
                units.bytes(100, 2),
                words!(
                    query_group_1_to_100 = "1 to 100\u{a0}{ $unit }",
                    unit = m,
                    note = "A group's heading, grouped by size. unit: a unit of size, as sizes are shown (MB, MiB)."
                )
                .say(),
                format!("size>=1{m} size<100{m}"),
            ),
            (
                units.bytes(1, 3),
                words!(
                    query_group_100_to_1 = "100\u{a0}{ $unit } to 1\u{a0}{ $next }",
                    unit = m,
                    next = g,
                    note = "A group's heading, grouped by size. unit and next: units of size, as sizes are shown (kB and MB)."
                )
                .say(),
                format!("size>=100{m} size<1{g}"),
            ),
        ],
        (
            words!(
                query_group_over_1 = "Over 1\u{a0}{ $unit }",
                unit = g,
                note = "A group's heading, grouped by size. unit: a unit of size, as sizes are shown (GB, GiB)."
            )
            .say(),
            format!("size>=1{g}"),
        ),
    )
}

/// By size: Empty, Under 100 kB, 100 kB to 1 MB, 1 to 100 MB, 100 MB to
/// 1 GB, Over 1 GB, in the units sizes are shown in; Folders, which are
/// not weighed, as having none. A place by how many files it holds, as
/// its size.
fn by_size() -> Keyer {
    let folders = Arc::new(Group::ranked(0, &folders_heading()).of_folders());
    let (sizes, (over_label, over_term)) = sizes();
    let groups: Vec<Arc<Group>> = sizes
        .iter()
        .zip(1..)
        .map(|((_, label, term), rank)| {
            Arc::new(Group::ranked(rank, label).with_term(term.clone()))
        })
        .collect();
    let below: Vec<u64> = sizes.iter().map(|(below, ..)| *below).collect();
    let over = Arc::new(Group::ranked(6, &over_label).with_term(over_term));
    Arc::new(move |e: &Of<'_>| {
        if !SortKey::Size.applies_to(e) {
            return Arc::clone(&folders);
        }
        let size = e
            .metadata
            .value(if e.is_place() {
                keys::FILES
            } else {
                keys::SIZE
            })
            .and_then(Value::as_u64)
            .unwrap_or(0);
        below
            .iter()
            .position(|below| size < *below)
            .map_or_else(|| Arc::clone(&over), |at| Arc::clone(&groups[at]))
    })
}

/// By a picture's size in pixels, in classes of megapixels; Folders, which
/// have none, first while folders come first; No dimensions for what says
/// none. No term: a filter cannot read a picture's size.
fn by_dimensions() -> Keyer {
    let classes = [
        (
            1_000_000,
            words!(
                query_group_under_1_mp = "Under 1\u{a0}MP",
                note = "A group's heading, grouped by a picture's dimensions: MP, megapixels."
            ),
        ),
        (
            4_000_000,
            words!(
                query_group_1_to_4_mp = "1 to 4\u{a0}MP",
                note = "A group's heading, grouped by a picture's dimensions: MP, megapixels."
            ),
        ),
        (
            12_000_000,
            words!(
                query_group_4_to_12_mp = "4 to 12\u{a0}MP",
                note = "A group's heading, grouped by a picture's dimensions: MP, megapixels."
            ),
        ),
        (
            24_000_000,
            words!(
                query_group_12_to_24_mp = "12 to 24\u{a0}MP",
                note = "A group's heading, grouped by a picture's dimensions: MP, megapixels."
            ),
        ),
    ];
    let folders = Arc::new(Group::ranked(0, &folders_heading()).of_folders());
    let none = Arc::new(Group::ranked(
        1,
        &words!(
            query_group_no_dimensions = "No dimensions",
            note = "A group's heading, grouped by a picture's dimensions: what says none."
        )
        .say(),
    ));
    let groups: Vec<Arc<Group>> = classes
        .iter()
        .zip(2..)
        .map(|((_, label), rank)| Arc::new(Group::ranked(rank, &label.say())))
        .collect();
    let classes: Vec<u64> = classes.iter().map(|(below, _)| *below).collect();
    let over = Arc::new(Group::ranked(
        6,
        &words!(
            query_group_over_24_mp = "Over 24\u{a0}MP",
            note = "A group's heading, grouped by a picture's dimensions: MP, megapixels."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        if !SortKey::Dimensions.applies_to(e) {
            return Arc::clone(&folders);
        }
        let Some(Value::List(sides)) = e.metadata.value(keys::DIMENSIONS) else {
            return Arc::clone(&none);
        };
        let side = |at: usize| sides.get(at).and_then(Value::as_u64).unwrap_or(0);
        let area = side(0).saturating_mul(side(1));
        classes
            .iter()
            .position(|below| area < *below)
            .map_or_else(|| Arc::clone(&over), |at| Arc::clone(&groups[at]))
    })
}

/// By how long a sound or a film lasts, in classes; Folders first while
/// folders come first; No duration for what says none. No term: a filter
/// cannot read a length.
fn by_duration() -> Keyer {
    let classes = [
        (
            60_000,
            words!(
                query_group_under_1_minute = "Under 1 minute",
                note = "A group's heading, grouped by how long a sound or a film lasts."
            ),
        ),
        (
            300_000,
            words!(
                query_group_1_to_5_minutes = "1 to 5 minutes",
                note = "A group's heading, grouped by how long a sound or a film lasts."
            ),
        ),
        (
            1_200_000,
            words!(
                query_group_5_to_20_minutes = "5 to 20 minutes",
                note = "A group's heading, grouped by how long a sound or a film lasts."
            ),
        ),
        (
            3_600_000,
            words!(
                query_group_20_minutes_to_1_hour = "20 minutes to 1 hour",
                note = "A group's heading, grouped by how long a sound or a film lasts."
            ),
        ),
    ];
    let folders = Arc::new(Group::ranked(0, &folders_heading()).of_folders());
    let none = Arc::new(Group::ranked(
        1,
        &words!(
            query_group_no_duration = "No duration",
            note =
                "A group's heading, grouped by how long a sound or a film lasts: what says none."
        )
        .say(),
    ));
    let groups: Vec<Arc<Group>> = classes
        .iter()
        .zip(2..)
        .map(|((_, label), rank)| Arc::new(Group::ranked(rank, &label.say())))
        .collect();
    let classes: Vec<u64> = classes.iter().map(|(below, _)| *below).collect();
    let over = Arc::new(Group::ranked(
        6,
        &words!(
            query_group_over_1_hour = "Over 1 hour",
            note = "A group's heading, grouped by how long a sound or a film lasts."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        if !SortKey::Duration.applies_to(e) {
            return Arc::clone(&folders);
        }
        let Some(ms) = e.metadata.value(keys::DURATION).and_then(Value::as_u64) else {
            return Arc::clone(&none);
        };
        classes
            .iter()
            .position(|below| ms < *below)
            .map_or_else(|| Arc::clone(&over), |at| Arc::clone(&groups[at]))
    })
}

/// What a sort by Dimensions or Duration reads of a file on demand
/// (ADR-131, 5): a picture's size from its header, a sound's or a film's
/// length from its boxes or ffprobe (`skwir_platform::facts`). Taken is
/// the geo plugin's. Out of Properties, which says them from the preview.
pub struct Facts;

impl skwir_core::fields::FieldProvider for Facts {
    fn id(&self) -> &'static str {
        "query.facts"
    }

    fn keys(&self) -> &'static [&'static str] {
        &[keys::DIMENSIONS, keys::DURATION]
    }

    fn accepts(&self, key: &str, name: &std::ffi::OsStr) -> bool {
        let Some(mime) = mime::by_name(name) else {
            return false;
        };
        let mime = mime.as_str();
        match key {
            // A drawing has no pixels of its own.
            keys::DIMENSIONS => mime.starts_with("image/") && !mime.starts_with("image/svg"),
            keys::DURATION => mime.starts_with("audio/") || mime.starts_with("video/"),
            _ => false,
        }
    }

    fn read(
        &self,
        key: &str,
        path: &std::path::Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Value>> {
        Ok(match key {
            keys::DIMENSIONS => skwir_platform::facts::dimensions(path)
                .map(|(width, height)| Value::List(vec![Value::UInt(width), Value::UInt(height)])),
            keys::DURATION => skwir_platform::facts::duration(path, cancel)?.map(Value::UInt),
            _ => None,
        })
    }
}

/// By rating, one group per rating, its stars said; Unrated first going
/// up.
fn by_rating() -> Keyer {
    let groups: Vec<Arc<Group>> = (0..=10u64)
        .map(|r| {
            Arc::new(if r == 0 {
                Group::ranked(
                    0,
                    &words!(
                        query_group_unrated = "Unrated",
                        note = "A group's heading, grouped by rating: files with no stars."
                    )
                    .say(),
                )
                .with_term("rating:0")
            } else {
                let stars = "★".repeat(usize::try_from(r / 2).unwrap_or(0))
                    + if r % 2 == 1 { "½" } else { "" };
                let value = if r % 2 == 1 {
                    format!("{}.5", r / 2)
                } else {
                    (r / 2).to_string()
                };
                Group::ranked(r, &stars).with_term(format!("rating:{value}"))
            })
        })
        .collect();
    Arc::new(move |e: &Of<'_>| {
        let r = e
            .metadata
            .value(keys::RATING)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(10);
        Arc::clone(&groups[usize::try_from(r).unwrap_or(0)])
    })
}

/// By what its repository says of it (ADR-134), the most pressing first:
/// in conflict, changed, staged, new, then Unchanged (what its repository
/// said is as the last commit has it), then Not known (what nothing was
/// said of: not read yet, not reached, in no repository), the ignored
/// last. No term keeps them: no query reads the state.
fn by_vcs() -> Keyer {
    use skwir_core::vcs::{Known, State};
    let group = |known: Known| {
        let label = match known {
            Known::Is(state) => state.words().say(),
            Known::Committed => words!(
                query_group_unchanged = "Unchanged",
                note = "A group's heading, grouped by Git state: files as the last commit has them."
            )
            .say(),
            Known::Not => words!(
                query_group_state_unknown = "Not known",
                note = "A group's heading, grouped by Git state: files nothing was said of (in no repository, not read yet)."
            )
            .say(),
        };
        Arc::new(Group::ranked(u64::from(State::rank(known)), &label))
    };
    let (unchanged, unknown) = (group(Known::Committed), group(Known::Not));
    let said: Vec<(State, Arc<Group>)> = State::ALL
        .into_iter()
        .map(|state| (state, group(Known::Is(state))))
        .collect();
    Arc::new(move |e: &Of<'_>| match State::known(e) {
        Known::Is(state) => said
            .iter()
            .find(|(known, _)| *known == state)
            .map_or_else(|| Arc::clone(&unknown), |(_, group)| Arc::clone(group)),
        Known::Committed => Arc::clone(&unchanged),
        Known::Not => Arc::clone(&unknown),
    })
}

/// By the whole set of tags a file has (`draft, print`), so no file is
/// shown twice; No tags first going up.
fn by_tags() -> Keyer {
    let made = Made::<String>::new();
    let none = Arc::new(Group::ranked(
        0,
        &words!(
            query_group_no_tags = "No tags",
            note = "A group's heading, grouped by tags: files with none."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| match tags_of(e) {
        Some(tags) => made.get(tags.clone(), || Group::new(1, &tags)),
        None => Arc::clone(&none),
    })
}

/// By the owner's name, or its number where the system names none. The
/// names are read once (`users::load`, on a worker): asked before they
/// are, an owner's number stands for its name, kept by no group, and the
/// keys say they changed once the names are there, so that an order made
/// meanwhile is made again with them.
fn by_owner(app: &Weak<App>, keys: &Keys) -> Keyer {
    if !skwir_platform::users::loaded() {
        read_names(app, keys);
    }
    let made = Made::<u64>::new();
    let none = Arc::new(Group::ranked(
        0,
        &words!(
            query_group_no_owner = "No owner",
            note = "A group's heading, grouped by owner: what says none."
        )
        .say(),
    ));
    Arc::new(move |e: &Of<'_>| {
        let Some(uid) = e.metadata.value(keys::UID).and_then(Value::as_u64) else {
            return Arc::clone(&none);
        };
        if !skwir_platform::users::loaded() {
            return Arc::new(Group::new(1, &uid.to_string()));
        }
        made.get(uid, || {
            let name = skwir_platform::users::user(uid).unwrap_or_else(|| uid.to_string());
            Group::new(1, &name)
        })
    })
}

/// Read the users' names: here, off the window's thread (a script's, an
/// agent's, a test's, which may wait on a file); from the window's, on a
/// worker, the keys saying they changed once read.
fn read_names(app: &Weak<App>, keys: &Keys) {
    if !skwir_core::scheduler::on_window_thread() {
        skwir_platform::users::load();
        return;
    }
    let Some(app) = app.upgrade() else {
        return;
    };
    let (changed, woken) = (Arc::clone(&keys.changed), app.weak());
    app.scheduler.spawn(
        Priority::Visible,
        "query.user-names",
        CancelToken::new(),
        move || {
            skwir_platform::users::load();
            changed.fetch_add(1, Ordering::AcqRel);
            if let Some(app) = woken.upgrade() {
                app.wake();
            }
        },
    );
}

/// The date groups of a day (ADR-130, 7), each the first that holds it,
/// counted on the user's calendar from `today`: the days each starts on,
/// newest first.
struct Dates {
    field: &'static str,
    /// What a query calls the field: `modified`.
    word: &'static str,
    today: i64,
    week: i64,
    last_week: i64,
    month: i64,
    last_month: i64,
    year: i64,
    /// The groups for the periods above, then In the future and No date.
    groups: Vec<Arc<Group>>,
    /// A group per year before this one.
    years: Made<i64>,
}

impl Dates {
    fn at(key: SortKey, cal: &Calendar, now: SystemTime) -> Self {
        let (field, word) = match key {
            SortKey::Taken => (keys::TAKEN, "taken"),
            SortKey::Created => (keys::CREATED, "created"),
            SortKey::Accessed => (keys::ACCESSED, "accessed"),
            SortKey::Deleted => (keys::TRASHED, "deleted"),
            _ => (keys::MODIFIED, "modified"),
        };
        let today = cal.day_of(now);
        let week = Period::Week.floor(today, cal);
        let month = Period::Month.floor(today, cal);
        let mut dates = Dates {
            field,
            word,
            today,
            week,
            last_week: week - 7,
            month,
            last_month: Period::Month.before(today, cal).0,
            year: Period::Year.floor(today, cal),
            groups: Vec::new(),
            years: Made(Mutex::new(HashMap::new())),
        };
        let yesterday = today - 1;
        let starts = [
            (
                words!(
                    query_group_today = "Today",
                    note = "A group's heading, grouped by a date."
                ),
                today,
                Some(today + 1),
            ),
            (
                words!(
                    query_group_yesterday = "Yesterday",
                    note = "A group's heading, grouped by a date."
                ),
                yesterday,
                Some(today),
            ),
            (
                words!(
                    query_group_earlier_this_week = "Earlier this week",
                    note = "A group's heading, grouped by a date: this week, before yesterday."
                ),
                week,
                Some(yesterday),
            ),
            (
                words!(
                    query_group_last_week = "Last week",
                    note = "A group's heading, grouped by a date."
                ),
                dates.last_week,
                Some(week.min(yesterday)),
            ),
            (
                words!(
                    query_group_earlier_this_month = "Earlier this month",
                    note = "A group's heading, grouped by a date: this month, before last week."
                ),
                month,
                Some(dates.last_week.min(yesterday)),
            ),
            (
                words!(
                    query_group_last_month = "Last month",
                    note = "A group's heading, grouped by a date."
                ),
                dates.last_month,
                Some(month.min(dates.last_week).min(yesterday)),
            ),
            (
                words!(
                    query_group_earlier_this_year = "Earlier this year",
                    note = "A group's heading, grouped by a date: this year, before last month."
                ),
                dates.year,
                Some(dates.last_month.min(dates.last_week).min(yesterday)),
            ),
        ];
        // What a group is ordered by: the day it starts on, then where it
        // stands among the groups, so two starting on one day stay two.
        for ((label, from, to), stands) in starts.into_iter().zip((2u8..=8).rev()) {
            let mut group = Group {
                key: Arc::from([stands]),
                ..Group::ranked(day_rank(from), &label.say())
            };
            group.term = to.and_then(|to| dates.term(from, to)).map(Arc::from);
            dates.groups.push(Arc::new(group));
        }
        let mut future = Group {
            key: Arc::from([9u8]),
            ..Group::ranked(
                day_rank(today + 1),
                &words!(
                    query_group_future = "In the future",
                    note = "A group's heading, grouped by a date: dates after today."
                )
                .say(),
            )
        };
        if field != keys::TRASHED && field != keys::TAKEN {
            future.term = Some(Arc::from(format!("{word}>today")));
        }
        dates.groups.push(Arc::new(future));
        dates.groups.push(Arc::new(Group::ranked(
            0,
            &words!(
                query_group_no_date = "No date",
                note = "A group's heading, grouped by a date: what says none."
            )
            .say(),
        )));
        dates
    }

    /// The query term that keeps the days from `from` to `to` (excluded):
    /// each group's from its own start to where the next newer one begins,
    /// so its term keeps its files alone; none for an empty one, nor in the
    /// trash, which a query has no field for.
    fn term(&self, from: i64, to: i64) -> Option<String> {
        let word = self.word;
        (to > from && self.says()).then(|| {
            if to - from == 1 {
                format!("{word}:{}", date_string(from))
            } else {
                format!("{word}:{}..{}", date_string(from), date_string(to - 1))
            }
        })
    }

    /// Whether a filter's term keeps a group's files: not in the trash,
    /// which a query has no field for, nor by when a picture was taken,
    /// which a filter cannot read of a file (only a search does).
    fn says(&self) -> bool {
        self.field != keys::TRASHED && self.field != keys::TAKEN
    }

    /// The day of `e`'s date: the user's day of its time, or the camera's
    /// clock's own day, which says no zone.
    fn day(&self, e: &Entry, cal: &Calendar) -> Option<i64> {
        let value = e.metadata.value(self.field)?;
        if self.field == keys::TAKEN {
            let clock = value.as_u64()?;
            let part = |by: u64| u32::try_from(clock / by % 100).ok();
            return skwir_platform::calendar::civil_day(
                i64::try_from(clock / 10_000_000_000).ok()?,
                part(100_000_000)?,
                part(1_000_000)?,
            );
        }
        Some(cal.day_of(value.as_time()?))
    }

    /// The group of `e`'s date.
    fn of(&self, e: &Entry, cal: &Calendar) -> Arc<Group> {
        let Some(day) = self.day(e, cal) else {
            return Arc::clone(&self.groups[8]);
        };
        let at = if day > self.today {
            7
        } else if day == self.today {
            0
        } else if day == self.today - 1 {
            1
        } else if day >= self.week {
            2
        } else if day >= self.last_week {
            3
        } else if day >= self.month {
            4
        } else if day >= self.last_month {
            5
        } else if day >= self.year {
            6
        } else {
            let year = skwir_platform::calendar::civil_from_days(day).0;
            return self.years.get(year, || {
                let (first, next) = Period::Year.around(day, cal);
                // Last year whole, but in January, when Last month and Last
                // week take its last days.
                let earliest = self.year.min(self.last_month).min(self.last_week);
                let term = if next <= earliest {
                    self.says().then(|| format!("{}:{year}", self.word))
                } else {
                    self.term(first, earliest)
                };
                // The year as the reader's locale writes it (`2024年`).
                let heading = Moment::on(year, 1, 1, Shape::Year)
                    .map_or_else(|| year.to_string(), Moment::said);
                Group {
                    key: Arc::from([1u8]),
                    term: term.map(Arc::from),
                    ..Group::ranked(day_rank(first), &heading)
                }
            });
        };
        Arc::clone(&self.groups[at])
    }
}

/// A day as a group's rank: later days higher, days before 1970 too.
fn day_rank(day: i64) -> u64 {
    u64::try_from(day.saturating_add(1 << 40)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::entry::{EntryId, EntryKind, FileId, Provenance};
    use skwir_platform::calendar::days_from_civil;

    fn entry(ino: u64, name: &str, kind: EntryKind) -> Entry {
        Entry::new(
            EntryId {
                file: FileId::new(1, ino),
                at: 0,
            },
            name,
            kind,
        )
    }

    /// The instant of `y-m-d h:m` on `cal`'s clock.
    fn at(cal: &Calendar, (y, m, d): (i64, u32, u32), h: u64, min: u64) -> SystemTime {
        cal.day_start(days_from_civil(y, m, d))
            + std::time::Duration::from_secs(h * 3600 + min * 60)
    }

    fn modified(t: Option<SystemTime>) -> Entry {
        let mut e = entry(1, "a", EntryKind::File);
        if let Some(t) = t {
            e.metadata
                .set(keys::MODIFIED, Value::Time(t), Provenance::Filesystem);
        }
        e
    }

    /// The label and term of the group of each date, by `keys` made on
    /// `cal` at `now`.
    fn date_groups(
        cal: &Calendar,
        now: SystemTime,
        dates: &[Option<SystemTime>],
    ) -> Vec<(String, Option<String>, u64)> {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Modified),
            cal,
            now,
        )
        .unwrap();
        let group = keys.group.unwrap();
        dates
            .iter()
            .map(|t| {
                let g = group(&Of::entry(&modified(*t)));
                (
                    g.label.to_string(),
                    g.term.as_deref().map(str::to_owned),
                    g.rank,
                )
            })
            .collect()
    }

    /// A type's group is kept while the words its name was read in are
    /// said: once they move (a language chosen), its name is read again,
    /// and one read while they moved is not kept.
    #[test]
    fn a_type_s_name_is_read_again_once_the_words_move() {
        let png = Mime("image/png".into());
        let named = || match type_group(&png) {
            TypeName::Named(group) => group,
            TypeName::Unnamed => panic!("the database names PNG"),
        };
        let first = named();
        assert!(Arc::ptr_eq(&first, &named()), "kept");
        skwir_core::locale::set_chosen(Some(words::PSEUDO.into()));
        assert!(known_type_group("image/png").is_none(), "forgotten");
        let again = named();
        assert!(!Arc::ptr_eq(&first, &again), "read again");
        assert!(Arc::ptr_eq(&again, &named()), "kept again");
        // As if the words moved while the name was read: what it read is
        // answered, not kept.
        *type_groups().write() = TypeGroups {
            said_at: words::revision() + 1,
            groups: HashMap::new(),
        };
        type_group(&png);
        assert!(type_groups().read().groups.is_empty(), "not kept");
        skwir_core::locale::set_chosen(None);
    }

    /// A type the database names not is headed as the Type column calls
    /// its files, by their extension, never by its id; its term still
    /// keeps the type's files.
    #[test]
    fn a_type_named_not_is_headed_by_its_files_extension() {
        *type_groups().write() = TypeGroups {
            said_at: words::revision(),
            groups: HashMap::from([("image/png".to_owned(), TypeName::Unnamed)]),
        };
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Type),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let chart = entry(1, "chart.png", EntryKind::File);
        let other = entry(2, "plan.png", EntryKind::File);
        let g = group(&Of::entry(&chart));
        assert_eq!(&*g.label, "PNG file");
        assert_eq!(g.term.as_deref(), Some("mime:image/png"));
        assert!(Arc::ptr_eq(&g, &group(&Of::entry(&other))), "one group");
        assert_eq!(&*mime::unnamed("Makefile").say(), "File");
    }

    /// On Wednesday 11 November 2026, weeks from Monday: each date in the
    /// first group that holds it, its term keeping that group's days alone,
    /// the groups newest first by rank, each boundary instant on its side.
    #[test]
    fn date_groups_on_a_wednesday() {
        let cal = Calendar::utc();
        let now = at(&cal, (2026, 11, 11), 15, 0);
        let t = |ymd, h, m| Some(at(&cal, ymd, h, m));
        let got = date_groups(
            &cal,
            now,
            &[
                t((2026, 11, 12), 0, 0),
                t((2026, 11, 11), 0, 0),
                t((2026, 11, 10), 23, 59),
                t((2026, 11, 9), 0, 0),
                t((2026, 11, 8), 23, 59),
                t((2026, 11, 2), 0, 0),
                t((2026, 11, 1), 12, 0),
                t((2026, 10, 31), 23, 59),
                t((2026, 10, 1), 0, 0),
                t((2026, 9, 30), 23, 59),
                t((2026, 1, 1), 0, 0),
                t((2025, 12, 31), 23, 59),
                t((2024, 6, 1), 0, 0),
                None,
            ],
        );
        let said: Vec<(&str, Option<&str>)> = got
            .iter()
            .map(|(label, term, _)| (label.as_str(), term.as_deref()))
            .collect();
        assert_eq!(
            said,
            [
                ("In the future", Some("modified>today")),
                ("Today", Some("modified:2026-11-11")),
                ("Yesterday", Some("modified:2026-11-10")),
                ("Earlier this week", Some("modified:2026-11-09")),
                ("Last week", Some("modified:2026-11-02..2026-11-08")),
                ("Last week", Some("modified:2026-11-02..2026-11-08")),
                ("Earlier this month", Some("modified:2026-11-01")),
                ("Last month", Some("modified:2026-10-01..2026-10-31")),
                ("Last month", Some("modified:2026-10-01..2026-10-31")),
                ("Earlier this year", Some("modified:2026-01-01..2026-09-30")),
                ("Earlier this year", Some("modified:2026-01-01..2026-09-30")),
                ("2025", Some("modified:2025")),
                ("2024", Some("modified:2024")),
                ("No date", None),
            ]
        );
        // Newest first by rank, as the order goes down.
        let ranks: Vec<u64> = got.iter().map(|(_, _, rank)| *rank).collect();
        assert!(ranks.windows(2).all(|w| w[0] >= w[1]), "{ranks:?}");
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Modified),
            &cal,
            now,
        )
        .unwrap();
        assert_eq!(keys.next_change, Some(at(&cal, (2026, 11, 12), 0, 0)));
    }

    /// A year before this one is headed as the reader's locale writes its
    /// years, its term the query language's, in every language.
    #[test]
    fn a_year_is_headed_as_the_locale_writes_years() {
        let cal = Calendar::utc();
        let now = at(&cal, (2026, 11, 11), 15, 0);
        // Its last instant on the calendar the groups count on.
        let late = Some(at(&cal, (2024, 12, 31), 23, 59));
        skwir_core::locale::set_formats(None, Some("ja".into()));
        let got = date_groups(&cal, now, &[late]);
        skwir_core::locale::set_formats(None, None);
        assert_eq!(
            got[0].0.as_str(),
            "2024\u{5e74}",
            "2024年, its term {:?}",
            got[0].1
        );
        assert_eq!(got[0].1.as_deref(), Some("modified:2024"));
    }

    /// On a Monday, yesterday was last week: Earlier this week holds
    /// nothing, and Last week's term leaves out the day Yesterday holds.
    #[test]
    fn on_a_monday_yesterday_was_last_week() {
        let cal = Calendar::utc();
        let now = at(&cal, (2026, 11, 9), 10, 0);
        let t = |ymd| Some(at(&cal, ymd, 12, 0));
        let got = date_groups(
            &cal,
            now,
            &[
                t((2026, 11, 8)),
                t((2026, 11, 7)),
                t((2026, 11, 1)),
                t((2026, 10, 30)),
            ],
        );
        assert_eq!(got[0].0, "Yesterday");
        assert_eq!(got[1].0, "Last week");
        assert_eq!(got[1].1.as_deref(), Some("modified:2026-11-02..2026-11-07"));
        assert_eq!(got[2].0, "Earlier this month");
        assert_eq!(got[2].1.as_deref(), Some("modified:2026-11-01"));
        assert_eq!(got[3].0, "Last month");
    }

    /// In January, last month and last week were last year: what they take
    /// of it is theirs (the 1st was in last week, so Earlier this month
    /// holds nothing), and the year's group and term the rest.
    #[test]
    fn in_january_last_month_was_last_year() {
        let cal = Calendar::utc();
        let now = at(&cal, (2027, 1, 5), 9, 0);
        let t = |ymd| Some(at(&cal, ymd, 12, 0));
        let got = date_groups(
            &cal,
            now,
            &[
                t((2027, 1, 1)),
                t((2026, 12, 29)),
                t((2026, 12, 15)),
                t((2026, 11, 30)),
            ],
        );
        let said: Vec<(&str, Option<&str>)> = got
            .iter()
            .map(|(label, term, _)| (label.as_str(), term.as_deref()))
            .collect();
        assert_eq!(
            said,
            [
                ("Last week", Some("modified:2026-12-28..2027-01-03")),
                ("Last week", Some("modified:2026-12-28..2027-01-03")),
                ("Last month", Some("modified:2026-12-01..2026-12-27")),
                ("2026", Some("modified:2026-01-01..2026-11-30")),
            ]
        );
    }

    /// The days are the user's: past midnight in Paris is today there,
    /// though it is still yesterday in UTC, and the groups change at
    /// Paris's next midnight.
    #[test]
    fn the_days_are_those_of_the_user_s_zone() {
        let paris =
            Calendar::named("Europe/Paris", skwir_platform::calendar::Weekday::Monday).unwrap();
        let now = at(&paris, (2026, 11, 11), 12, 0);
        let just_after = at(&paris, (2026, 11, 11), 0, 30);
        let just_before = at(&paris, (2026, 11, 10), 23, 59);
        let got = date_groups(&paris, now, &[Some(just_after), Some(just_before)]);
        assert_eq!(got[0].0, "Today");
        assert_eq!(got[1].0, "Yesterday");
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Modified),
            &paris,
            now,
        )
        .unwrap();
        assert_eq!(keys.next_change, Some(at(&paris, (2026, 11, 12), 0, 0)));
    }

    #[test]
    fn size_groups_at_their_bounds() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Size),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let of = |size: u64| {
            let mut e = entry(1, "f", EntryKind::File);
            e.metadata
                .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
            group(&Of::entry(&e)).label.to_string()
        };
        // A thousand to the next, as sizes are shown by default
        // (ADR-139, 9).
        let sizes = [
            0,
            1,
            99_999,
            100_000,
            999_999,
            1_000_000,
            100_000_000,
            999_999_999,
            1_000_000_000,
        ];
        assert_eq!(
            sizes.map(of),
            [
                "Empty",
                "Under 100\u{a0}kB",
                "Under 100\u{a0}kB",
                "100\u{a0}kB to 1\u{a0}MB",
                "100\u{a0}kB to 1\u{a0}MB",
                "1 to 100\u{a0}MB",
                "100\u{a0}MB to 1\u{a0}GB",
                "100\u{a0}MB to 1\u{a0}GB",
                "Over 1\u{a0}GB",
            ]
        );
        // Each group kept by a term in the same units, which the query reads
        // as it means it.
        let term = |size: u64| {
            let mut e = entry(1, "f", EntryKind::File);
            e.metadata
                .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
            group(&Of::entry(&e)).term.as_deref().map(str::to_owned)
        };
        assert_eq!(term(500_000).as_deref(), Some("size>=100kB size<1MB"));
        assert_eq!(crate::lang::parse_bytes("100kB").unwrap(), 100_000);
        assert_eq!(crate::lang::parse_bytes("1MB").unwrap(), 1_000_000);
        let folders = group(&Of::entry(&entry(2, "d", EntryKind::Directory)));
        assert!(folders.folders);
        assert_eq!(&*folders.label, "Folders");
    }

    /// By name: the first letter as the name sort folds it, 0–9, Other; a
    /// letter of another script its own; one group shared by the names it
    /// holds.
    #[test]
    fn name_groups_fold_as_the_name_sort() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Size,
            Some(SortKey::Name),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let of = |name: &str| group(&Of::entry(&entry(1, name, EntryKind::File)));
        let labels = ["École", "eagle", "2024 report", "_draft", "Ωmega", "zebra"]
            .map(|name| of(name).label.to_string());
        assert_eq!(labels, ["E", "E", "0–9", "Other", "Ω", "Z"]);
        assert!(Arc::ptr_eq(&of("École"), &of("eagle")));
        // Other, then 0–9, then the letters.
        assert!(of("_draft").standing(&of("2024")).is_lt());
        assert!(of("2024").standing(&of("apple")).is_lt());
        assert!(of("apple").standing(&of("zebra")).is_lt());
    }

    /// By name in any script: Japanese's kana rows, Katakana with
    /// Hiragana, the kanji no heading is for after the letters; Traditional
    /// Chinese's counts of strokes, in their order.
    #[test]
    fn name_groups_take_the_headings_of_every_script() {
        use skwir_core::collate::Collation;
        let ja: &'static Collation = Box::leak(Box::new(Collation::of(Some("ja-JP"))));
        let group = by_name_in(ja);
        let of = |name: &str| group(&Of::entry(&entry(1, name, EntryKind::File)));
        let labels = [
            "かな",
            "ガイド",
            "さくら",
            "漢字",
            "東京",
            "Apple",
            "_x",
            "10",
        ]
        .map(|name| of(name).label.to_string());
        assert_eq!(
            labels,
            [
                "か",
                "か",
                "さ",
                "Other letters",
                "Other letters",
                "A",
                "Other",
                "0–9"
            ]
        );
        assert!(Arc::ptr_eq(&of("かな"), &of("ガイド")));
        assert!(Arc::ptr_eq(&of("漢字"), &of("東京")));
        assert!(
            of("さくら").standing(&of("漢字")).is_lt(),
            "after the letters"
        );
        assert!(of("Apple").standing(&of("漢字")).is_lt());
        let tw: &'static Collation = Box::leak(Box::new(Collation::of(Some("zh-TW"))));
        let group = by_name_in(tw);
        let of = |name: &str| group(&Of::entry(&entry(1, name, EntryKind::File)));
        let labels = ["一", "二", "龍", "Apple"].map(|name| of(name).label.to_string());
        assert_eq!(labels, ["1 stroke", "2 strokes", "16 strokes", "A"]);
        assert!(of("一").standing(&of("二")).is_lt());
        assert!(of("二").standing(&of("龍")).is_lt());
    }

    /// What its repository said of a file: its state, Unchanged where it
    /// said the file is as committed, and Not known where nothing was said
    /// (not read yet, in no repository): never Unchanged.
    #[test]
    fn a_file_nobody_said_anything_of_is_not_known_never_unchanged() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Vcs),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let of = |value: Option<Value>| {
            let mut e = entry(1, "f", EntryKind::File);
            if let Some(value) = value {
                e.metadata.set(keys::VCS, value, Provenance::Filesystem);
            }
            let g = group(&Of::entry(&e));
            (g.label.to_string(), g.rank)
        };
        let changed = of(Some(Value::Text("changed".into())));
        let unchanged = of(Some(Value::Bool(false)));
        let unknown = of(None);
        assert_eq!(changed.0, "Changed");
        assert_eq!(unchanged.0, "Unchanged");
        assert_eq!(unknown.0, "Not known");
        let gone = of(Some(Value::Text(skwir_core::vcs::UNKNOWN.into())));
        assert_eq!(gone, unknown, "what was said went");
        assert!(changed.1 < unchanged.1 && unchanged.1 < unknown.1);
        assert!(unknown.1 < of(Some(Value::Text("ignored".into()))).1);
    }

    #[test]
    fn ratings_say_their_stars() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Rating),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let of = |rating: Option<u64>| {
            let mut e = entry(1, "f", EntryKind::File);
            if let Some(r) = rating {
                e.metadata
                    .set(keys::RATING, Value::UInt(r), Provenance::User);
            }
            let g = group(&Of::entry(&e));
            (g.label.to_string(), g.term.as_deref().map(str::to_owned))
        };
        assert_eq!(of(None), ("Unrated".into(), Some("rating:0".into())));
        assert_eq!(of(Some(7)), ("★★★½".into(), Some("rating:3.5".into())));
        assert_eq!(of(Some(10)), ("★★★★★".into(), Some("rating:5".into())));
    }

    #[test]
    fn extensions_fold_case_and_folders_have_none() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Extension),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let of = |name: &str, kind| group(&Of::entry(&entry(1, name, kind)));
        assert!(Arc::ptr_eq(
            &of("a.PNG", EntryKind::File),
            &of("b.png", EntryKind::File)
        ));
        assert_eq!(&*of("a.PNG", EntryKind::File).label, ".png");
        assert_eq!(
            of("a.PNG", EntryKind::File).term.as_deref(),
            Some("ext:png")
        );
        assert_eq!(&*of("README", EntryKind::File).label, "No extension");
        assert!(of("v1.2", EntryKind::Directory).folders);
    }

    /// By folder, one group a folder, in their paths' order, called by its
    /// path from the home folder; a file not in one (a place) in none.
    #[test]
    fn each_folder_is_a_group_of_its_own() {
        let keys = keys_at(
            &Weak::new(),
            SortKey::Name,
            Some(SortKey::Folder),
            &Calendar::utc(),
            SystemTime::now(),
        )
        .unwrap();
        let group = keys.group.unwrap();
        let file = entry(1, "a.txt", EntryKind::File);
        let home = skwir_core::os_path::home().unwrap_or_else(|| std::path::PathBuf::from("/h"));
        let of = |folder: Option<&std::path::Path>| {
            group(&Of {
                entry: &file,
                folder,
                collated: None,
            })
        };
        let notes = home.join("notes");
        let maps = home.join("notes/maps");
        assert!(Arc::ptr_eq(&of(Some(&notes)), &of(Some(&notes))));
        assert!(!Arc::ptr_eq(&of(Some(&notes)), &of(Some(&maps))));
        assert_eq!(&*of(Some(&maps)).label, "~/notes/maps");
        assert!(of(Some(&notes)).standing(&of(Some(&maps))).is_lt());
        assert_eq!(of(Some(&notes)).term, None, "a filter cannot say where");
        assert_eq!(&*of(None).label, "No folder");
        assert!(of(None).standing(&of(Some(&notes))).is_lt());
    }

    /// A sort by name, by size or by a date needs no keys of its own; by
    /// kind, it does.
    #[test]
    fn keys_only_where_something_needs_them() {
        let made = |sort, group| {
            keys_at(
                &Weak::new(),
                sort,
                group,
                &Calendar::utc(),
                SystemTime::now(),
            )
        };
        assert!(made(SortKey::Name, None).is_none());
        assert!(made(SortKey::Modified, None).is_none());
        let kind = made(SortKey::Kind, None).unwrap();
        assert!(kind.value.is_some() && kind.group.is_none());
        let folder = (kind.value.unwrap())(&Of::entry(&entry(1, "d", EntryKind::Directory)));
        assert_eq!(&*folder.label, "Folders");
        assert!(folder.folders);
    }
}
