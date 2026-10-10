//! The `tags:` location (ADR-032): the tags of the files under a folder,
//! browsed as folders are. `tags:/Places/France` is the tag
//! `Places/France`: its tags are places (`EntryId::place`), listed as
//! folders, each counting the files within it; its own files are those
//! tagged with it exactly, or everything within it with `deep` ("Include
//! Sub-Tags"). What narrows it stays while its trail is walked: files that
//! also have a tag (`with`), none that have one (`without`), a kind of
//! file, a year. It is of a folder (`in`, the home folder when omitted),
//! whose tagged files the index knows (`index.rs`): shown at once where a
//! count covered the folder, then checked; a first visit counts it, the
//! listing complete once it is counted.
//!
//! It holds references: files dropped or pasted on a tag are given it,
//! and moved from another tag lose that one (`tags.drop`); Delete takes
//! its tag off them (`tags.untag`); New Folder makes a tag in it, kept
//! while bibli runs until a file is given it (`tags.new`: nothing on disk
//! can hold a tag no file has); F2 on a tag renames it everywhere.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::Invocation;
use bibli_core::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};
use bibli_core::location::{
    Crumb, EmptyState, EventSink, ListingEvent, Location, LocationProvider, Narrowing, OpenContext,
    OpenLocation, Toggle, Trail, Verb, Verbs, escape, unescape,
};
use bibli_core::metadata::{
    TagRef, clean_tag, same_tag, tag_leaf, tag_parent, tag_under, tag_within,
};
use bibli_core::os_path::PathArg;
use bibli_core::scheduler::Priority;
use bibli_platform::mime::Kind;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use crate::index::{TagIndex, stable_hash};

pub const SCHEME: &str = "tags";

/// A tag location, as its text says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Address {
    /// The tag shown; the top, every tag, when `None`.
    pub tag: Option<String>,
    /// The folder whose tags these are; the home folder when `None`.
    pub scope: Option<PathBuf>,
    /// Only files that also have a tag within each.
    pub with: Vec<String>,
    /// Only files that have no tag within any.
    pub without: Vec<String>,
    /// Only files of one of these kinds (by their names).
    pub kinds: Vec<Kind>,
    /// Only files modified in one of these years.
    pub years: Vec<i64>,
    /// Every file within the tag, not only those tagged with it exactly.
    pub deep: bool,
}

fn push_tag(list: &mut Vec<String>, tag: &str) {
    let tag = clean_tag(tag);
    if !tag.is_empty() && !list.iter().any(|t| same_tag(t, &tag)) {
        list.push(tag);
    }
}

impl Address {
    pub fn parse(rest: &str) -> Address {
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let text = |s: &str| String::from_utf8_lossy(&unescape(s)).into_owned();
        let tag = clean_tag(&path.split('/').map(text).collect::<Vec<_>>().join("/"));
        let mut address = Address {
            tag: (!tag.is_empty()).then_some(tag),
            ..Address::default()
        };
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "in" => {
                    let path = PathBuf::from(bibli_core::os_path::from_bytes(unescape(value)));
                    address.scope = scope_of(path);
                }
                "with" => push_tag(&mut address.with, &text(value)),
                "without" => push_tag(&mut address.without, &text(value)),
                "kind" => {
                    if let Some(kind) = Kind::from_name(&text(value))
                        && !address.kinds.contains(&kind)
                    {
                        address.kinds.push(kind);
                    }
                }
                "year" => {
                    if let Ok(year) = text(value).parse::<i64>()
                        && !address.years.contains(&year)
                    {
                        address.years.push(year);
                    }
                }
                "deep" => address.deep = true,
                _ => {}
            }
        }
        address
    }

    /// What follows `tags:`.
    pub fn text(&self) -> String {
        let mut out = String::from("/");
        if let Some(tag) = &self.tag {
            let parts: Vec<String> = tag.split('/').map(|p| escape(p.as_bytes())).collect();
            out.push_str(&parts.join("/"));
        }
        let mut params: Vec<String> = Vec::new();
        if let Some(scope) = &self.scope {
            params.push(format!(
                "in={}",
                escape(scope.as_os_str().as_encoded_bytes())
            ));
        }
        params.extend(
            self.with
                .iter()
                .map(|t| format!("with={}", escape(t.as_bytes()))),
        );
        params.extend(
            self.without
                .iter()
                .map(|t| format!("without={}", escape(t.as_bytes()))),
        );
        params.extend(self.kinds.iter().map(|k| format!("kind={}", k.name())));
        params.extend(self.years.iter().map(|y| format!("year={y}")));
        if self.deep {
            params.push("deep".into());
        }
        if !params.is_empty() {
            out.push('?');
            out.push_str(&params.join("&"));
        }
        out
    }

    pub fn location(&self) -> Location {
        Location::new(SCHEME, self.text())
    }

    /// Every tag of `folder`: the top of its tags (`tags:/?in=folder`,
    /// `tags:/` for the home folder).
    pub fn of_folder(folder: &Path) -> Address {
        Address {
            scope: scope_of(folder.to_path_buf()),
            ..Address::default()
        }
    }

    /// The tag `tag` (the top when `None`) of the same folder, narrowed the
    /// same way: where a step along the trail goes.
    pub fn at(&self, tag: Option<String>) -> Address {
        Address {
            tag,
            ..self.clone()
        }
    }

    /// The folder whose tags these are.
    pub fn root(&self) -> Option<PathBuf> {
        self.scope.clone().or_else(std::env::home_dir)
    }

    /// Whether it narrows by anything but its tag.
    pub fn narrowed(&self) -> bool {
        !self.with.is_empty()
            || !self.without.is_empty()
            || !self.kinds.is_empty()
            || !self.years.is_empty()
    }

    /// Its trail: the folder it is of, Tags, the categories and the tag,
    /// then what narrows it, each pill leading where it does not.
    pub fn trail(&self) -> Trail {
        let mut crumbs = vec![Crumb::new("Tags", self.at(None).location())];
        if let Some(tag) = &self.tag {
            let mut prefix = String::new();
            for part in tag.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                crumbs.push(
                    Crumb::new(part, self.at(Some(prefix.clone())).location())
                        .of_tag(TagRef::of(&prefix)),
                );
            }
        }
        let mut filters = Vec::new();
        for (i, tag) in self.with.iter().enumerate() {
            let mut without = self.clone();
            without.with.remove(i);
            filters.push(
                Crumb::new(format!("with {}", tag_leaf(tag)), without.location())
                    .of_tag(TagRef::of(tag)),
            );
        }
        for (i, tag) in self.without.iter().enumerate() {
            let mut without = self.clone();
            without.without.remove(i);
            filters.push(
                Crumb::new(format!("not {}", tag_leaf(tag)), without.location())
                    .of_tag(TagRef::lacking(tag)),
            );
        }
        for (i, kind) in self.kinds.iter().enumerate() {
            let mut without = self.clone();
            without.kinds.remove(i);
            filters.push(Crumb::new(kind.title(), without.location()));
        }
        for (i, year) in self.years.iter().enumerate() {
            let mut without = self.clone();
            without.years.remove(i);
            filters.push(Crumb::new(
                format!("modified in {year}"),
                without.location(),
            ));
        }
        let switched = Address {
            deep: !self.deep,
            ..self.clone()
        };
        if self.deep {
            filters.push(Crumb::new("with sub-tags", switched.location()));
        }
        Trail {
            base: self.root(),
            crumbs,
            filters,
            toggles: vec![Toggle {
                label: "Include Sub-Tags".into(),
                on: self.deep,
                location: switched.location(),
            }],
            // A tag holds the tags within it, the top of a folder's tags
            // every tag.
            holds_places: true,
            one_folder: false,
        }
    }

    /// What it says when it lists nothing: the top of a folder's tags,
    /// that no file there has one; a tag, how to give it; narrowed, that
    /// nothing is what it is narrowed to.
    pub fn empty_state(&self) -> EmptyState {
        let narrowed = !(self.with.is_empty()
            && self.without.is_empty()
            && self.kinds.is_empty()
            && self.years.is_empty());
        if narrowed {
            EmptyState::new(
                "No file here is what it is narrowed to",
                "Take a narrowing off in the address.",
            )
        } else if self.tag.is_none() {
            EmptyState::new(
                "No file here has a tag",
                "Give files tags with Tags… in their menu, or drop them on a tag in the sidebar.",
            )
        } else {
            EmptyState::new(
                "No file here has this tag",
                "Drop files here to give them the tag.",
            )
        }
    }

    /// What gestures mean here (ADR-032).
    pub fn verbs(&self) -> Verbs {
        let location = self.location().to_string();
        let here = self.tag.as_deref();
        let ask = |title: String, placeholder: &str, text: &str, command: &str, args, field| {
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": title,
                    "placeholder": placeholder,
                    "text": text,
                    "command": command,
                    "args": args,
                    "field": field,
                }),
            )
        };
        let mut set = Vec::new();
        if !self.kinds.is_empty() {
            set.push("kind".to_owned());
        }
        if !self.years.is_empty() {
            set.push("modified".to_owned());
        }
        Verbs {
            // At the top, tags dropped come out of their categories.
            drop: Some(Invocation::with_args(
                "tags.drop",
                &serde_json::json!({ "location": location }),
            )),
            remove: Some(Verb::new(
                match here {
                    Some(tag) => format!("Remove from “{}”", tag_leaf(tag)),
                    None => "Remove All Tags".into(),
                },
                Invocation::with_args("tags.untag", &serde_json::json!({ "location": location })),
            )),
            create: Some(Verb::new(
                "New Tag…",
                ask(
                    match here {
                        Some(tag) => format!("New tag in “{}”", tag_leaf(tag)),
                        None => "New tag".into(),
                    },
                    "Name",
                    "",
                    "tags.new",
                    serde_json::json!({ "location": location }),
                    "name",
                ),
            )),
            rename: here.map(|tag| {
                Verb::new(
                    "Rename Tag…",
                    ask(
                        format!("Rename the tag “{tag}” on every file"),
                        "New name (an existing tag's merges them)",
                        tag,
                        "tags.rename",
                        serde_json::json!({
                            "tag": tag,
                            "root": self.root().map(PathArg::from),
                        }),
                        "to",
                    ),
                )
            }),
            narrow: Some(Narrowing {
                command: Invocation::with_args(
                    "tags.narrow",
                    &serde_json::json!({ "location": location }),
                ),
                tags: here
                    .map(str::to_owned)
                    .into_iter()
                    .chain(self.with.iter().cloned())
                    .collect(),
                set,
            }),
            // A tag copied into a folder: a folder of its files there.
            export: here.map(|_| {
                Invocation::with_args("tags.export", &serde_json::json!({ "location": location }))
            }),
            restore: None,
            tools: Vec::new(),
        }
    }
}

/// What a tag exported into a folder makes (`tags.export`): a folder of
/// its own files, named as the tag, and one inside it for each tag within
/// it, as deep as they go: each file copied where it is listed, under a
/// new name when one there has its name already.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Export {
    /// The folders to make, outermost first.
    pub folders: Vec<PathBuf>,
    /// Each file and the folder and name it is copied to.
    pub copies: Vec<(PathBuf, PathBuf, std::ffi::OsString)>,
}

/// The export of `address`'s tag (narrowed as it is) into `dest`, from
/// the tagged files under its folder; `taken` says whether a name is
/// taken in `dest` already (the folder made gets another).
pub fn export(
    address: &Address,
    files: &[Tagged],
    dest: &Path,
    taken: impl Fn(&Path) -> bool,
) -> Option<Export> {
    let tag = address.tag.as_deref()?;
    let mut out = Export::default();
    let name = tag_leaf(tag);
    let mut top = dest.join(name);
    let mut n = 2;
    while taken(&top) {
        top = dest.join(format!("{name} ({n})"));
        n += 1;
    }
    let mut stack: Vec<(String, PathBuf)> = vec![(tag.to_owned(), top)];
    while let Some((tag, folder)) = stack.pop() {
        let here = Address {
            tag: Some(tag),
            deep: false,
            ..address.clone()
        };
        let contents = contents(&here, files, &[]);
        out.folders.push(folder.clone());
        let mut names: HashSet<std::ffi::OsString> = HashSet::new();
        for file in contents.files {
            let Some(name) = file.file_name() else {
                continue;
            };
            let mut unique = name.to_owned();
            let mut n = 2;
            while !names.insert(unique.clone()) {
                let path = Path::new(name);
                let stem = path.file_stem().unwrap_or(name).to_string_lossy();
                unique = match path.extension() {
                    Some(ext) => format!("{stem} ({n}).{}", ext.to_string_lossy()).into(),
                    None => format!("{stem} ({n})").into(),
                };
                n += 1;
            }
            out.copies.push((file, folder.clone(), unique));
        }
        for (child, _) in contents.places.into_iter().rev() {
            let leaf = tag_leaf(&child).to_owned();
            stack.push((child, folder.join(leaf)));
        }
    }
    Some(out)
}

/// `path` as a scope: none for the home folder, which is the default.
fn scope_of(path: PathBuf) -> Option<PathBuf> {
    let home = std::env::home_dir();
    let path = match path.strip_prefix("~") {
        Ok(rest) => home.as_ref()?.join(rest),
        Err(_) => path,
    };
    (path.is_absolute() && Some(&path) != home.as_ref()).then_some(path)
}

/// A tagged file as a tag location counts it.
pub struct Tagged {
    pub path: PathBuf,
    pub tags: Vec<String>,
    /// The year it was modified, when a filter asks for it.
    pub year: Option<i64>,
}

/// What a tag location lists.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Contents {
    /// The tags in it, each with how many of its files are within it.
    pub places: Vec<(String, usize)>,
    /// Beside each place, a few of the pictures within it, by their names
    /// the last first (a camera's numbers: the newest): its tile's mosaic.
    pub pictures: Vec<Vec<PathBuf>>,
    pub files: Vec<PathBuf>,
}

/// How many pictures a place's tile shows.
pub const MOSAIC: usize = 4;

/// Whether `path`'s name says it is a picture.
fn is_picture(path: &Path) -> bool {
    path.file_name()
        .and_then(bibli_platform::mime::by_name)
        .is_some_and(|m| bibli_platform::mime::is_kind(&m, Kind::Image))
}

/// Whether `file` passes what narrows `address`.
fn passes(address: &Address, file: &Tagged) -> bool {
    let within = |category: &String| file.tags.iter().any(|t| tag_within(t, category));
    address.with.iter().all(within)
        && !address.without.iter().any(within)
        && (address.kinds.is_empty()
            || file
                .path
                .file_name()
                .and_then(bibli_platform::mime::by_name)
                .is_some_and(|m| {
                    address
                        .kinds
                        .iter()
                        .any(|k| bibli_platform::mime::is_kind(&m, *k))
                }))
        && (address.years.is_empty() || file.year.is_some_and(|y| address.years.contains(&y)))
}

/// What `address` lists of `files` (each with a tag within its tag, or
/// any tag at the top), and `drafts`, tags made and not given yet.
pub fn contents(address: &Address, files: &[Tagged], drafts: &[String]) -> Contents {
    // The tags inside, by their names ignoring case: the spelling met
    // first, and the files within each, once.
    let mut places: Vec<(String, usize)> = Vec::new();
    let mut pictures: Vec<Vec<PathBuf>> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    let inside = |file: &&Tagged| match &address.tag {
        Some(category) => file.tags.iter().any(|t| tag_within(t, category)),
        None => !file.tags.is_empty(),
    };
    for file in files.iter().filter(inside).filter(|f| passes(address, f)) {
        let mut seen: Vec<String> = Vec::new();
        let mut here = false;
        for tag in &file.tags {
            let child = match &address.tag {
                Some(category) => {
                    let Some(rest) = tag_under(tag, category) else {
                        continue;
                    };
                    // The tag within the category, by its first name: the
                    // category as the tag spells it, then that name. None
                    // (the category itself): a file of its own.
                    let Some(next) = rest.split('/').find(|name| !name.is_empty()) else {
                        here = true;
                        continue;
                    };
                    format!("{}/{next}", &tag[..tag.len() - rest.len()])
                }
                None => tag.split('/').next().unwrap_or(tag).to_owned(),
            };
            if !seen.iter().any(|s| same_tag(s, &child)) {
                seen.push(child);
            }
        }
        let picture = !seen.is_empty() && is_picture(&file.path);
        for child in seen {
            let at = *by_name.entry(child.to_lowercase()).or_insert_with(|| {
                places.push((child, 0));
                pictures.push(Vec::new());
                places.len() - 1
            });
            places[at].1 += 1;
            if picture {
                // The last names first, as many as a tile shows.
                let list = &mut pictures[at];
                let name = file.path.file_name();
                let after = list.partition_point(|p| p.file_name() > name);
                if after < MOSAIC {
                    list.insert(after, file.path.clone());
                    list.truncate(MOSAIC);
                }
            }
        }
        if here || address.deep {
            out.push(file.path.clone());
        }
    }
    for draft in drafts {
        let inside = match &address.tag {
            Some(category) => tag_parent(draft).is_some_and(|p| same_tag(p, category)),
            None => tag_parent(draft).is_none(),
        };
        if inside && !by_name.contains_key(&draft.to_lowercase()) {
            places.push((draft.clone(), 0));
            pictures.push(Vec::new());
        }
    }
    Contents {
        places,
        pictures,
        files: out,
    }
}

/// The entry a file is listed as, before it is stat-ed in full
/// (hydration reads the rest): its identity and what it is. `None` when
/// it is gone.
fn entry_at(path: &Path) -> Option<Entry> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let kind = if meta.is_dir() {
        EntryKind::Directory
    } else if meta.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    // A tag's files are from many folders: told apart by their paths.
    let id = EntryId::at_path(bibli_core::fs::stat::identity(path, &meta)?, path);
    Some(Entry::new(id, path.file_name()?.to_owned(), kind))
}

/// A place's pictures as its tile draws them (`keys::PLACE_PICTURES`):
/// each its path, identity and modification time, for a thumbnail. Stats
/// them: on a worker.
fn tile_of(pictures: &[PathBuf]) -> Value {
    Value::List(
        pictures
            .iter()
            .filter_map(|path| {
                let meta = std::fs::metadata(path).ok()?;
                let id = bibli_core::fs::stat::identity(path, &meta)?;
                Some(Value::List(vec![
                    Value::Text(path.to_string_lossy().as_ref().into()),
                    Value::UInt(id.dev),
                    Value::UInt(id.ino),
                    Value::Time(meta.modified().ok()?),
                ]))
            })
            .collect(),
    )
}

/// A place's identity: its tag, whatever its case.
pub fn place_id(tag: &str) -> EntryId {
    EntryId::place(stable_hash(&[tag.to_lowercase().as_bytes()]))
}

/// What a scope's count stands at.
#[derive(Clone, Debug, Default)]
struct Scope {
    /// A count or a check of it runs.
    running: bool,
    /// Counted or checked since bibli started: what the index says of it
    /// holds.
    done: bool,
    /// A plan changed something under it while its count ran: what the
    /// count read before may be older than what the index learned from
    /// the plan, so it is checked again once the count ends (cheaply: by
    /// change times).
    touched: bool,
}

/// What every open tag location shares: the index, the counts under way,
/// and the tags made that no file has yet.
pub struct Places {
    index: Arc<TagIndex>,
    app: Weak<App>,
    lifetime: CancelToken,
    open: Mutex<Vec<Weak<Open>>>,
    scopes: Mutex<HashMap<PathBuf, Scope>>,
    /// Tags made here (`tags.new`), with the folder they were made in.
    drafts: Mutex<Vec<(PathBuf, String)>>,
    /// The index was found full once: said once.
    told_full: std::sync::atomic::AtomicBool,
}

impl Places {
    pub fn new(index: Arc<TagIndex>, app: &App, lifetime: CancelToken) -> Arc<Self> {
        Arc::new(Places {
            index,
            app: app.weak(),
            lifetime,
            open: Mutex::new(Vec::new()),
            scopes: Mutex::new(HashMap::new()),
            drafts: Mutex::new(Vec::new()),
            told_full: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// The index changed: every open tag location lists again.
    pub fn touch(self: &Arc<Self>) {
        self.relist(None);
    }

    /// A plan changed `paths`: a count running over one of them is checked
    /// again once it ends (`Scope::touched`).
    pub fn changed(&self, paths: &[&Path]) {
        for (root, scope) in self.scopes.lock().iter_mut() {
            if scope.running && paths.iter().any(|p| p.starts_with(root)) {
                scope.touched = true;
            }
        }
    }

    /// List again the open tag locations of `root` (all with `None`), each
    /// on a worker, once for changes close together.
    fn relist(self: &Arc<Self>, root: Option<&Path>) {
        let open: Vec<Arc<Open>> = {
            let mut open = self.open.lock();
            open.retain(|o| o.strong_count() > 0);
            open.iter().filter_map(Weak::upgrade).collect()
        };
        for location in open {
            if root.is_none_or(|r| location.root == r) {
                location.relist_soon();
            }
        }
    }

    /// Count `root`'s tags unless counted since bibli started, or checked
    /// what changed when a count covers it; `again` checks anew.
    fn ensure(self: &Arc<Self>, root: &Path, again: bool) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        {
            let mut scopes = self.scopes.lock();
            let scope = scopes.entry(root.to_path_buf()).or_default();
            if scope.running || (scope.done && !again) {
                return;
            }
            scope.running = true;
        }
        let (this, root) = (Arc::clone(self), root.to_path_buf());
        app.scheduler.spawn(
            Priority::Normal,
            "tags.count",
            self.lifetime.child(),
            move || {
                let Some(app) = this.app.upgrade() else {
                    return;
                };
                let covered = this.index.covering(&root).map(|c| c.since());
                let result = crate::count::run(&app, &this.index, &root, covered, &this.lifetime);
                match result {
                    Ok(Some((since, dirs, counted))) => {
                        tracing::debug!(
                            checked = covered.is_some(),
                            folders = counted.folders,
                            read = counted.read,
                            "tags: counted for a tag location"
                        );
                        this.index.cover(&root, since, &dirs);
                    }
                    Ok(None) => return,
                    Err(error) => tracing::debug!("tags: a tag location's count: {error:#}"),
                }
                let touched = this.scopes.lock().get_mut(&root).is_some_and(|scope| {
                    scope.running = false;
                    scope.done = true;
                    std::mem::take(&mut scope.touched)
                });
                if touched {
                    this.ensure(&root, true);
                }
                if this.index.is_full()
                    && !this
                        .told_full
                        .swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    app.notify_about_setting(
                        bibli_core::app::NotifyLevel::Warning,
                        "Some tagged files are left out of the tags: the room bibli gives them is full",
                        crate::index::LIMIT_SETTING,
                    );
                }
                this.relist(Some(&root));
                app.wake();
            },
        );
    }

    /// Whether `root`'s tags were counted or checked since bibli
    /// started: what the index says of them holds. A tag location of it
    /// is complete then; until, it shows what the index knew, checking.
    fn done(&self, root: &Path) -> bool {
        self.scopes.lock().get(root).is_some_and(|s| s.done)
    }

    /// `tags.counted`: whether `root`'s tags are being counted, and as of
    /// when a count covers them.
    pub fn counted(&self, root: &Path) -> serde_json::Value {
        let scope = self.scopes.lock().get(root).cloned().unwrap_or_default();
        let since = self
            .index
            .covering(root)
            .and_then(|c| c.since().duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        serde_json::json!({
            "root": PathArg::from(root),
            "counting": scope.running,
            "counted": since,
            "partial": self.index.is_full(),
        })
    }

    /// A tag made in `root`'s tags (`tags.new`), shown until a file has it.
    pub fn draft(self: &Arc<Self>, root: &Path, tag: &str) {
        {
            let mut drafts = self.drafts.lock();
            if !drafts
                .iter()
                .any(|(r, t)| r.as_path() == root && same_tag(t, tag))
            {
                drafts.push((root.to_path_buf(), tag.to_owned()));
            }
        }
        self.relist(Some(root));
    }

    /// The tagged files under `root` within `address`'s tag (every one
    /// at the top), with their years when it is narrowed by one. Reads the
    /// index and stats: on a worker.
    pub fn tagged(&self, address: &Address, root: &Path) -> Vec<Tagged> {
        let calendar = (!address.years.is_empty()).then(bibli_platform::calendar::Calendar::system);
        self.index
            .files_under(root, address.tag.as_deref())
            .into_iter()
            .map(|(path, tags)| {
                let year = calendar.as_ref().and_then(|calendar| {
                    let modified = std::fs::symlink_metadata(&path).ok()?.modified().ok()?;
                    Some(calendar.year_of(modified))
                });
                Tagged { path, tags, year }
            })
            .collect()
    }

    fn drafts_of(&self, root: &Path) -> Vec<String> {
        self.drafts
            .lock()
            .iter()
            .filter(|(r, _)| r.as_path() == root)
            .map(|(_, t)| t.clone())
            .collect()
    }

    /// Drafts that a file now has are tags: no longer drafts.
    fn forget_given(&self, root: &Path, given: &[(String, usize)]) {
        self.drafts.lock().retain(|(r, t)| {
            r.as_path() != root || !given.iter().any(|(g, n)| *n > 0 && same_tag(g, t))
        });
    }
}

/// What an open tag location sent: its places and files, by identity,
/// each file with the tags the index knew it by.
#[derive(Default)]
struct Sent {
    /// Each place's tag, files and pictures, as sent.
    places: HashMap<EntryId, (String, usize, Vec<PathBuf>)>,
    /// Each place's tile, as sent (`keys::PLACE_PICTURES`).
    tiles: HashMap<EntryId, Value>,
    files: HashMap<EntryId, PathBuf>,
    tags: HashMap<EntryId, Vec<String>>,
    /// Every file it ever sent: one sent again (its tag given back) is
    /// read at once, as views may have asked already.
    ever: HashSet<EntryId>,
    complete: bool,
}

/// Its relisting: one at a time, again once if asked meanwhile.
#[derive(Default)]
struct Relisting {
    running: bool,
    again: bool,
}

/// One open tag location.
pub struct Open {
    location: Location,
    address: Address,
    root: PathBuf,
    rx: Receiver<ListingEvent>,
    sink: EventSink,
    ctx: OpenContext,
    places: Arc<Places>,
    sent: Mutex<Sent>,
    relisting: Mutex<Relisting>,
    me: Weak<Open>,
}

impl Open {
    /// List again on a worker, unless a relisting runs: then once more
    /// after it.
    fn relist_soon(&self) {
        {
            let mut r = self.relisting.lock();
            if r.running {
                r.again = true;
                return;
            }
            r.running = true;
        }
        let Some(me) = self.me.upgrade() else { return };
        self.ctx.scheduler.spawn(
            Priority::Visible,
            "tags.location",
            self.ctx.cancel.child(),
            move || {
                // However the relisting ends, a panic included, it ends:
                // else the location would never list again.
                struct Ended<'a>(&'a Open);
                impl Drop for Ended<'_> {
                    fn drop(&mut self) {
                        self.0.relisting.lock().running = false;
                    }
                }
                let _ended = Ended(&me);
                loop {
                    me.relist();
                    let mut r = me.relisting.lock();
                    if !r.again || me.ctx.cancel.is_cancelled() {
                        return;
                    }
                    r.again = false;
                }
            },
        );
    }

    /// What the index says it holds now, sent as what changed since the
    /// last time. Reads the index and stats new files: on a worker.
    fn relist(&self) {
        let index = &self.places.index;
        let tagged = self.places.tagged(&self.address, &self.root);
        let drafts = self.places.drafts_of(&self.root);
        let contents = contents(&self.address, &tagged, &drafts);
        self.places.forget_given(&self.root, &contents.places);
        if self.ctx.cancel.is_cancelled() {
            return;
        }
        // The files not sent yet, identified by a stat outside the lock.
        let known: HashSet<PathBuf> = self.sent.lock().files.values().cloned().collect();
        let mut new_files: Vec<(PathBuf, Entry)> = Vec::new();
        let mut gone_paths: Vec<PathBuf> = Vec::new();
        for path in contents.files.iter().filter(|p| !known.contains(*p)) {
            match entry_at(path) {
                Some(entry) => new_files.push((path.clone(), entry)),
                None => gone_paths.push(path.clone()),
            }
        }
        if !gone_paths.is_empty() {
            // Gone since the index learned them.
            index.learn(gone_paths.into_iter().map(|p| (p, Vec::new())).collect());
        }
        let provenance = Provenance::Plugin {
            name: crate::ID.into(),
        };
        // A category holding one tag at a time is a field.
        let fields = self
            .places
            .app
            .upgrade()
            .map(|app| crate::fields::exclusive(&app))
            .unwrap_or_default();
        // The places whose pictures changed: those stat-ed for their tiles,
        // outside the lock.
        let tiles: HashMap<EntryId, Value> = {
            let before: HashMap<EntryId, Vec<PathBuf>> = self
                .sent
                .lock()
                .places
                .iter()
                .map(|(id, (_, _, pictures))| (*id, pictures.clone()))
                .collect();
            contents
                .places
                .iter()
                .zip(&contents.pictures)
                .filter(|((tag, _), pictures)| {
                    !pictures.is_empty() && before.get(&place_id(tag)) != Some(*pictures)
                })
                .map(|((tag, _), pictures)| (place_id(tag), tile_of(pictures)))
                .collect()
        };
        let wanted: HashSet<&PathBuf> = contents.files.iter().collect();
        let mut sent = self.sent.lock();
        let mut added: Vec<Entry> = Vec::new();
        let mut removed: Vec<EntryId> = Vec::new();
        // Places: new, gone, or counting another number of files, or
        // showing other pictures.
        let mut places_now: HashMap<EntryId, (String, usize, Vec<PathBuf>)> = HashMap::new();
        for ((tag, files), pictures) in contents.places.iter().zip(&contents.pictures) {
            let id = place_id(tag);
            let now = (tag.clone(), *files, pictures.clone());
            let same = sent.places.get(&id) == Some(&now);
            places_now.insert(id, now);
            if same {
                continue;
            }
            let mut entry = Entry::new(id, tag_leaf(tag), EntryKind::Directory);
            entry
                .metadata
                .set(keys::FILES, Value::UInt(*files as u64), provenance.clone());
            entry.metadata.set(
                keys::PLACE_TAG,
                Value::Text(tag.as_str().into()),
                provenance.clone(),
            );
            let kind = if fields.iter().any(|f| same_tag(f, tag)) {
                "Field"
            } else {
                "Tag"
            };
            entry.metadata.set(
                keys::PLACE_KIND,
                Value::Text(kind.into()),
                provenance.clone(),
            );
            // Pictures unchanged: the tile sent with them.
            let tile = tiles.get(&id).or_else(|| sent.tiles.get(&id)).cloned();
            if let Some(tile) = tile.filter(|_| !pictures.is_empty()) {
                entry
                    .metadata
                    .set(keys::PLACE_PICTURES, tile, provenance.clone());
            }
            added.push(entry);
        }
        removed.extend(
            sent.places
                .keys()
                .filter(|id| !places_now.contains_key(id))
                .copied(),
        );
        sent.tiles.retain(|id, _| places_now.contains_key(id));
        sent.tiles.extend(tiles);
        sent.places = places_now;
        // Files: those no longer listed go, the new ones come.
        let gone: Vec<EntryId> = sent
            .files
            .iter()
            .filter(|(_, path)| !wanted.contains(path))
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            sent.files.remove(id);
            sent.tags.remove(id);
        }
        removed.extend(gone);
        let mut again: Vec<EntryId> = Vec::new();
        for (path, entry) in new_files {
            if sent.files.contains_key(&entry.id) {
                continue;
            }
            if !sent.ever.insert(entry.id) {
                again.push(entry.id);
            }
            sent.files.insert(entry.id, path);
            added.push(entry);
        }
        // Files listed whose tags changed since: read again, so their rows
        // show what they have now.
        let tags_of: HashMap<&PathBuf, &Vec<String>> =
            tagged.iter().map(|t| (&t.path, &t.tags)).collect();
        let mut changed: Vec<EntryId> = Vec::new();
        let files: Vec<(EntryId, PathBuf)> = sent
            .files
            .iter()
            .map(|(id, path)| (*id, path.clone()))
            .collect();
        for (id, path) in files {
            let Some(now) = tags_of.get(&path) else {
                continue;
            };
            match sent.tags.insert(id, (*now).clone()) {
                Some(before) if before != **now => changed.push(id),
                _ => {}
            }
        }
        if !removed.is_empty() {
            self.sink.send(ListingEvent::Removed(removed));
        }
        if !added.is_empty() {
            self.sink.send(ListingEvent::Added(added));
        }
        if !sent.complete && self.places.done(&self.root) {
            sent.complete = true;
            self.sink.send(ListingEvent::Complete);
        }
        drop(sent);
        changed.extend(again);
        if !changed.is_empty() {
            self.hydrate(&changed, Priority::Visible);
        }
    }
}

impl OpenLocation for Open {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        let items: Vec<(EntryId, PathBuf)> = {
            let sent = self.sent.lock();
            ids.iter()
                .filter_map(|id| sent.files.get(id).map(|p| (*id, p.clone())))
                .collect()
        };
        if items.is_empty() {
            return;
        }
        // A job for each slice, side by side on the workers; what is gone
        // is sent as removed by the hydration itself.
        for items in items.chunks(bibli_core::fs::HYDRATE_JOB) {
            let items = items.to_vec();
            let sink = self.sink.clone();
            let metadata = Arc::clone(&self.ctx.metadata);
            let cancel = self.ctx.cancel.child();
            self.ctx
                .scheduler
                .spawn(priority, "tags.location.stat", cancel.clone(), move || {
                    bibli_core::fs::hydrate_paths(&items, &metadata, &sink, &cancel);
                });
        }
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.sent.lock().files.get(&id).cloned()
    }

    fn location_of(&self, id: EntryId) -> Option<Location> {
        let sent = self.sent.lock();
        let (tag, _, _) = sent.places.get(&id)?;
        Some(self.address.at(Some(tag.clone())).location())
    }

    /// F5: what changed since the last count is read again.
    fn refresh(&self) {
        self.places.ensure(&self.root, true);
        self.relist_soon();
    }

    fn close(&self) {
        self.ctx.cancel.cancel();
    }
}

/// The provider of `tags:` locations.
pub struct Provider {
    pub places: Arc<Places>,
}

impl LocationProvider for Provider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let address = Address::parse(&location.rest);
        let root = address
            .root()
            .ok_or_else(|| anyhow::anyhow!("no home folder to find tags in"))?;
        let (sink, rx) = ctx.channel();
        let open = Arc::new_cyclic(|me| Open {
            location: location.clone(),
            address,
            root: root.clone(),
            rx,
            sink,
            ctx,
            places: Arc::clone(&self.places),
            sent: Mutex::new(Sent::default()),
            relisting: Mutex::new(Relisting::default()),
            me: me.clone(),
        });
        self.places.open.lock().push(Arc::downgrade(&open));
        open.relist_soon();
        self.places.ensure(&root, false);
        Ok(open)
    }

    fn title(&self, location: &Location) -> Option<String> {
        let address = Address::parse(&location.rest);
        Some(match &address.tag {
            Some(tag) => tag_leaf(tag).to_owned(),
            None => "Tags".to_owned(),
        })
    }

    fn trail(&self, location: &Location) -> Option<Trail> {
        Some(Address::parse(&location.rest).trail())
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("tag")
    }

    fn empty_state(&self, location: &Location) -> Option<EmptyState> {
        Some(Address::parse(&location.rest).empty_state())
    }

    fn places(&self, location: &Location) -> Vec<Crumb> {
        let address = Address::parse(&location.rest);
        let Some(root) = address.root() else {
            return Vec::new();
        };
        let found = self.places.index.files_under(&root, address.tag.as_deref());
        let tagged: Vec<Tagged> = found
            .into_iter()
            .map(|(path, tags)| Tagged {
                path,
                tags,
                year: None,
            })
            .collect();
        let narrowed = Address {
            years: Vec::new(),
            ..address.clone()
        };
        let mut places = contents(&narrowed, &tagged, &self.places.drafts_of(&root)).places;
        places.sort_by_cached_key(|(tag, _)| {
            bibli_core::collate::sort_key(std::ffi::OsStr::new(tag_leaf(tag)))
        });
        places
            .into_iter()
            .map(|(tag, _)| Crumb::new(tag_leaf(&tag), address.at(Some(tag.clone())).location()))
            .collect()
    }

    fn verbs(&self, location: &Location) -> Verbs {
        Address::parse(&location.rest).verbs()
    }

    /// A folder's tags, first in its menu of what it holds, as its trail
    /// goes on into them (`Photos › Tags`): narrowed as the trail shown
    /// has them, when it goes on into them.
    fn places_of(&self, folder: &Path, along: Option<&Location>) -> Vec<Crumb> {
        let plain = Address::of_folder(folder);
        let tags = along
            .filter(|along| &*along.scheme == SCHEME)
            .map(|along| Address::parse(&along.rest))
            .filter(|shown| shown.tag.is_none() && shown.scope == plain.scope)
            .unwrap_or(plain);
        vec![Crumb::new("Tags", tags.location())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, tags: &[&str]) -> Tagged {
        Tagged {
            path: PathBuf::from(path),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            year: None,
        }
    }

    fn at(tag: Option<&str>) -> Address {
        Address {
            tag: tag.map(str::to_owned),
            ..Address::default()
        }
    }

    // Unix's paths.
    #[cfg(unix)]
    #[test]
    fn a_tag_exported_is_a_folder_of_its_files_and_one_per_tag_within() {
        let files = [
            file("/p/a/one.jpg", &["Places/France"]),
            file("/p/b/one.jpg", &["Places/France"]),
            file("/p/paris.jpg", &["Places/France/Paris"]),
            file("/p/tokyo.jpg", &["Places/Japan"]),
        ];
        let out = export(&at(Some("Places/France")), &files, Path::new("/out"), |p| {
            p == Path::new("/out/France")
        })
        .unwrap();
        assert_eq!(
            out.folders,
            [
                PathBuf::from("/out/France (2)"),
                PathBuf::from("/out/France (2)/Paris")
            ],
            "a name taken gets another"
        );
        let names: Vec<(String, String)> = out
            .copies
            .iter()
            .map(|(from, to, name)| {
                (
                    from.display().to_string(),
                    to.join(name).display().to_string(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                (
                    "/p/a/one.jpg".to_owned(),
                    "/out/France (2)/one.jpg".to_owned()
                ),
                (
                    "/p/b/one.jpg".to_owned(),
                    "/out/France (2)/one (2).jpg".to_owned()
                ),
                (
                    "/p/paris.jpg".to_owned(),
                    "/out/France (2)/Paris/paris.jpg".to_owned()
                ),
            ]
        );
        assert!(export(&at(None), &files, Path::new("/out"), |_| false).is_none());
    }

    /// Empty, the top of a folder's tags says no file there has one, a
    /// tag how to give it, and either narrowed that nothing is what it is
    /// narrowed to.
    #[test]
    fn an_empty_tag_location_says_why() {
        let said = |text: &str| Address::parse(text).empty_state().title;
        assert_eq!(said("/"), "No file here has a tag");
        assert_eq!(said("/Places/France"), "No file here has this tag");
        assert_eq!(
            said("/Places?with=family"),
            "No file here is what it is narrowed to"
        );
        assert_eq!(
            said("/?year=2024"),
            "No file here is what it is narrowed to"
        );
    }

    // Unix's paths.
    #[cfg(unix)]
    #[test]
    fn an_address_reads_back_what_it_wrote() {
        let address = Address {
            tag: Some("Places/Été & co?".into()),
            scope: Some("/media/stick/Photos 100%".into()),
            with: vec!["family".into(), "a=b".into()],
            without: vec!["Status/Draft".into()],
            kinds: vec![Kind::Image],
            years: vec![2024],
            deep: true,
        };
        let text = address.text();
        assert!(text.starts_with("/Places/Été %26 co%3F?in=/media/stick/Photos 100%25"));
        assert_eq!(Address::parse(&text), address);
        assert_eq!(
            Address::parse("/ Places / France /?with=Family&with=family&bogus"),
            Address {
                tag: Some("Places/France".into()),
                with: vec!["Family".into()],
                ..Address::default()
            },
            "a tag cleaned, a filter once whatever its case"
        );
        assert_eq!(Address::parse("/").tag, None);
        if let Some(home) = std::env::home_dir() {
            let given = format!("/x?in={}", home.display());
            assert_eq!(
                Address::parse(&given).scope,
                None,
                "the home folder is the default"
            );
        }
    }

    #[test]
    fn a_tag_lists_its_tags_as_places_and_its_own_files() {
        let files = [
            file("/p/paris.jpg", &["Places/France/Paris", "family"]),
            file("/p/lyon.jpg", &["Places/France/Lyon"]),
            file("/p/france.jpg", &["Places/France", "Places/France/Paris"]),
            file("/p/tokyo.jpg", &["places/Japan"]),
            file("/p/work.txt", &["work"]),
        ];
        let top = contents(&at(None), &files, &[]);
        assert_eq!(
            top.places,
            [
                ("Places".into(), 4),
                ("family".into(), 1),
                ("work".into(), 1)
            ]
        );
        assert!(top.files.is_empty(), "the top lists tags only");

        let places = contents(&at(Some("Places")), &files, &[]);
        assert_eq!(
            places.places,
            [("Places/France".into(), 3), ("places/Japan".into(), 1)],
            "each file once per place, whatever the case"
        );
        assert!(places.files.is_empty(), "no file is tagged Places exactly");

        let france = contents(&at(Some("Places/France")), &files, &[]);
        assert_eq!(
            france.places,
            [
                ("Places/France/Paris".into(), 2),
                ("Places/France/Lyon".into(), 1)
            ]
        );
        assert_eq!(france.files, [PathBuf::from("/p/france.jpg")]);

        let deep = Address {
            deep: true,
            ..at(Some("Places/France"))
        };
        assert_eq!(
            contents(&deep, &files, &[]).files.len(),
            3,
            "everything within"
        );
    }

    /// A tag written by another application with an empty category or a
    /// trailing separator lists as the tag it means, and never cuts a
    /// name inside a character.
    #[test]
    fn a_tag_spelled_loosely_lists_as_the_tag_it_means() {
        let files = [
            file("/p/a.jpg", &["Lieux//Été"]),
            file("/p/b.jpg", &["Lieux/"]),
        ];
        let lieux = contents(&at(Some("Lieux")), &files, &[]);
        assert_eq!(lieux.places, [("Lieux/Été".into(), 1)]);
        assert_eq!(lieux.files, [PathBuf::from("/p/b.jpg")]);
    }

    #[test]
    fn what_narrows_a_tag_narrows_its_places_and_its_files() {
        let files = [
            file("/p/paris.jpg", &["Places/France/Paris", "family"]),
            file("/p/lyon.jpg", &["Places/France/Lyon"]),
            file("/p/tokyo.jpg", &["Places/Japan", "family"]),
        ];
        let family = Address {
            with: vec!["family".into()],
            ..at(Some("Places"))
        };
        assert_eq!(
            contents(&family, &files, &[]).places,
            [("Places/France".into(), 1), ("Places/Japan".into(), 1)]
        );
        let not_family = Address {
            without: vec!["family".into()],
            ..at(Some("Places/France"))
        };
        assert_eq!(
            contents(&not_family, &files, &[]).places,
            [("Places/France/Lyon".into(), 1)],
            "Paris has no file left, so it is not listed"
        );
        let pictures = Address {
            kinds: vec![Kind::Video],
            ..at(Some("Places"))
        };
        assert!(contents(&pictures, &files, &[]).places.is_empty());
    }

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_place_shows_a_few_of_its_pictures_the_last_names_first() {
        let files: Vec<Tagged> = (1..=6)
            .map(|n| file(&format!("/p/IMG_{n:04}.jpg"), &["Places/France"]))
            .chain([file("/p/notes.txt", &["Places/France"])])
            .chain([file("/p/z.png", &["Places/Japan"])])
            .collect();
        let c = contents(&at(Some("Places")), &files, &[]);
        let france = c
            .places
            .iter()
            .position(|(t, _)| t == "Places/France")
            .unwrap();
        let names: Vec<String> = c.pictures[france]
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "IMG_0006.jpg",
                "IMG_0005.jpg",
                "IMG_0004.jpg",
                "IMG_0003.jpg"
            ],
            "four, the last names first, no text file"
        );
        let japan = c
            .places
            .iter()
            .position(|(t, _)| t == "Places/Japan")
            .unwrap();
        assert_eq!(c.pictures[japan].len(), 1);
    }

    #[test]
    fn a_tag_made_and_not_given_shows_where_it_was_made() {
        let files = [file("/p/a.jpg", &["Places/France"])];
        let drafts = vec!["Places/Spain".to_owned(), "Elsewhere".to_owned()];
        let places = contents(&at(Some("Places")), &files, &drafts).places;
        assert_eq!(
            places,
            [("Places/France".into(), 1), ("Places/Spain".into(), 0)]
        );
        let top = contents(&at(None), &files, &drafts).places;
        assert_eq!(top, [("Places".into(), 1), ("Elsewhere".into(), 0)]);
    }

    #[test]
    fn a_folder_s_tags_are_a_place_of_it() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            Address::of_folder(&home).location(),
            Location::new(SCHEME, "/"),
            "the home folder's are every tag, as All Tags opens them"
        );
        let photos = home.join("Photos");
        let of = Address::of_folder(&photos);
        assert_eq!(Address::parse(&of.text()), of);
        assert_eq!(of.scope.as_deref(), Some(photos.as_path()));
        assert_eq!(of.tag, None);
        assert_eq!(of.trail().base.as_deref(), Some(photos.as_path()));
    }

    #[test]
    fn its_trail_keeps_what_narrows_it_and_each_pill_leads_without_it() {
        let address = Address {
            with: vec!["family".into()],
            deep: true,
            ..at(Some("Places/France"))
        };
        let trail = address.trail();
        let labels: Vec<&str> = trail.crumbs.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Tags", "Places", "France"]);
        let tags: Vec<Option<TagRef>> = trail.crumbs.iter().map(|c| c.tag.clone()).collect();
        assert_eq!(
            tags,
            [
                None,
                Some(TagRef::of("Places")),
                Some(TagRef::of("Places/France"))
            ],
            "each tag's crumb stands for it, by its full name; Tags for none"
        );
        assert_eq!(
            Address::parse(&trail.crumbs[1].location.rest),
            Address {
                tag: Some("Places".into()),
                ..address.clone()
            },
            "going up keeps what narrows it"
        );
        let pills: Vec<&str> = trail.filters.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(pills, ["with family", "with sub-tags"]);
        let tags: Vec<Option<TagRef>> = trail.filters.iter().map(|c| c.tag.clone()).collect();
        assert_eq!(tags, [Some(TagRef::of("family")), None]);
        assert_eq!(
            Address::parse(&trail.filters[0].location.rest).with,
            Vec::<String>::new()
        );
        assert!(trail.toggles[0].on);
        assert_eq!(trail.parent(), Some(trail.crumbs[1].location.clone()));
    }
}
