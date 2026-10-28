//! Locations and streaming listings.
//!
//! A location is a string: a path, or `scheme:rest` for anything a plugin
//! provides (queries, trash, recent, ...). Opening one yields an
//! [`OpenLocation`] that streams [`ListingEvent`]s over a channel. The UI
//! applies them to a [`Listing`] (or its own model) and never waits.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId};
use crate::os_path::PathArg;
use crate::scheduler::{Priority, Scheduler};

/// The scheme used for plain filesystem paths.
pub const PATH_SCHEME: &str = "path";

/// A parsed location string.
///
/// A path location keeps its exact path: a name that is not valid UTF-8 has
/// no faithful string form, so `rest` then holds a display form and the path
/// itself travels beside it (serialized as bytes, see [`crate::os_path`]).
/// Build path locations with [`Location::from_path`] and read them with
/// [`Location::path`]; never round-trip one through `to_string`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Location {
    /// `path` for filesystem paths, otherwise a plugin's scheme.
    pub scheme: Arc<str>,
    /// Everything after the scheme; the path itself for `path` (a display
    /// form when the path is not valid UTF-8).
    pub rest: String,
    /// The exact path, only when `rest` cannot hold it.
    #[serde(default, rename = "path", skip_serializing_if = "Option::is_none")]
    raw: Option<PathArg>,
}

impl Location {
    /// A location of a plugin's `scheme`.
    pub fn new(scheme: &str, rest: impl Into<String>) -> Self {
        Location {
            scheme: scheme.into(),
            rest: rest.into(),
            raw: None,
        }
    }

    /// Parse `s`. Absolute or relative paths, `~`, and `scheme:rest` are all accepted.
    /// A single letter before the colon is treated as a Windows drive, not a scheme.
    pub fn parse(s: &str) -> Self {
        if let Some((scheme, rest)) = s.split_once(':')
            && scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Location::new(scheme, rest);
        }
        Location::new(PATH_SCHEME, s)
    }

    /// [`Location::parse`] for text that arrived as an OS string (a command
    /// argument, see [`crate::os_path`]): what is not UTF-8 can only be a path.
    pub fn parse_os(s: &OsStr) -> Self {
        match s.to_str() {
            Some(text) => Location::parse(text),
            None => Location::from_path(s),
        }
    }

    /// A path location, exact whatever bytes the path holds.
    pub fn from_path(p: impl AsRef<Path>) -> Self {
        let p = p.as_ref();
        match p.to_str() {
            Some(text) => Location::new(PATH_SCHEME, text),
            None => Location {
                scheme: PATH_SCHEME.into(),
                rest: p.to_string_lossy().into_owned(),
                raw: Some(PathArg::from(p)),
            },
        }
    }

    pub fn is_path(&self) -> bool {
        &*self.scheme == PATH_SCHEME
    }

    /// Whether `other` is the same place: a folder by its path (`~` and the
    /// exact bytes resolved), anything else as written.
    pub fn same_place(&self, other: &Location) -> bool {
        match (self.path(), other.path()) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self == other,
            _ => false,
        }
    }

    /// This location as a command's `location` argument, which
    /// `parse_os` reads back as it: a folder's exact path, whatever bytes
    /// it holds, anything else its text.
    pub fn arg(&self) -> PathArg {
        match self.path() {
            Some(path) => PathArg(path),
            None => PathArg(PathBuf::from(self.to_string())),
        }
    }

    /// The filesystem path, with `~` expanded, for `path` locations.
    pub fn path(&self) -> Option<PathBuf> {
        if !self.is_path() {
            return None;
        }
        if let Some(raw) = &self.raw {
            return Some(raw.to_path_buf());
        }
        let r = &self.rest;
        if r == "~" || r.starts_with("~/") || (cfg!(windows) && r.starts_with("~\\")) {
            let home = crate::os_path::home()?;
            return Some(if r == "~" { home } else { home.join(&r[2..]) });
        }
        Some(PathBuf::from(r))
    }
}

/// `text` written into a location's text with what would end it or read
/// otherwise escaped (`%`, `?`, `&`, `=`, `#`, control characters), and
/// bytes that are not UTF-8: the rest stays readable, as typed. A tag's
/// name, a folder's path in a location of a plugin's (`tags:`, `trash:`).
pub fn escape(text: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    for chunk in text.utf8_chunks() {
        for c in chunk.valid().chars() {
            if matches!(c, '%' | '?' | '&' | '=' | '#') || c.is_control() {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    let _ = write!(out, "%{b:02X}");
                }
            } else {
                out.push(c);
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The value of a hex digit, any case.
fn hex_digit(c: u8) -> Option<u8> {
    char::from(c)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// What `escape` wrote, as bytes again.
pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Two hex digits, nothing else: `from_str_radix` takes a sign too.
        if bytes[i] == b'%'
            && let Some(&[high, low]) = bytes.get(i + 1..i + 3)
            && let (Some(high), Some(low)) = (hex_digit(high), hex_digit(low))
        {
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_path() {
            f.write_str(&self.rest)
        } else {
            write!(f, "{}:{}", self.scheme, self.rest)
        }
    }
}

/// What a listing's answer rests on when it is not the files as they are
/// now: an index as it stood when it was last brought up to date
/// (ADR-050). A search of `/` that plocate answers is as of its database's
/// update, so what was made, renamed or removed since is not in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsOf {
    /// What answered, in the user's words: "plocate's database".
    pub by: String,
    /// When it was last brought up to date; `None` when that cannot be
    /// told.
    pub updated: Option<std::time::SystemTime>,
}

/// What an open location streams. Every variant is a batch.
#[derive(Clone, Debug)]
pub enum ListingEvent {
    /// New entries, or for an id already present its replacement. A
    /// replacement that carries no stat under the same name (a re-listing)
    /// keeps the metadata the listing already has.
    Added(Vec<Entry>),
    /// Entries whose name, kind, or metadata changed. Ids the listing does
    /// not hold are ignored: a late stat must not resurrect a removed entry.
    Updated(Vec<Entry>),
    /// Entries that no longer exist here.
    Removed(Vec<EntryId>),
    /// Entries renamed: each the entry an id now is, under its new name
    /// (a file's name changed, its file the same). The listing tells its
    /// holders the old id is the new one (`Listing::take_renamed`), so a
    /// selection follows it. An id the listing does not hold arrives as an
    /// addition.
    Renamed(Vec<(EntryId, Entry)>),
    /// The initial listing is complete; further events come from watching.
    Complete,
    /// The listing failed or was interrupted.
    Error(String),
    /// The location itself changed, not what it lists: the folder's
    /// permissions, times or attributes (its tags). Whoever describes the
    /// location reads it again (`Listing::itself_version`).
    Changed,
    /// What the answer rests on: an index as of its last update, or
    /// `None` for the files as they are (a walk). Said by each run of a
    /// listing that may rest on either, so a run that walks takes back
    /// what an earlier one said.
    AsOf(Option<AsOf>),
    /// The location is reached at another, which is shown instead, in
    /// its place in the history: an address connected to a mount shows
    /// the mounted folder (ADR-077). Sent once, as the last event.
    Redirect(Location),
    /// The folder listed moved, and the listing followed it: it lists it
    /// at this path from now on. A path location is then reached there:
    /// its holder shows it at that address, in its place in the history,
    /// with what it opened in place inside it (`Listing::moved_to`).
    Moved(PathBuf),
}

/// Called after events are queued so a sleeping UI can wake and pump.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a provider gets when a location is opened.
#[derive(Clone)]
pub struct OpenContext {
    pub scheduler: Scheduler,
    pub cancel: CancelToken,
    /// Preferred batch size for `Added` events.
    pub batch_size: usize,
    /// Wake hook; providers should send through [`OpenContext::channel`] which calls it.
    pub waker: Option<Waker>,
    /// Metadata providers to run after stat (see [`crate::metadata`]).
    pub metadata: Arc<crate::metadata::MetadataProviders>,
    /// Fields read on demand (see [`crate::fields`]): a search's `near:`.
    pub fields: Arc<crate::fields::Fields>,
    /// Indexes a search asks first (see [`crate::hints`]): a `tag:`.
    pub hints: Arc<crate::hints::Hints>,
    /// Files beside a file (see [`crate::companions`]): a listing marks
    /// them.
    pub companions: Arc<crate::companions::Companions>,
}

impl OpenContext {
    /// An event channel whose sender wakes the UI after each send.
    pub fn channel(&self) -> (EventSink, Receiver<ListingEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            EventSink {
                tx,
                waker: self.waker.clone(),
            },
            rx,
        )
    }
}

/// Sender half of a listing channel. Never blocks; wakes the UI.
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<ListingEvent>,
    waker: Option<Waker>,
}

impl EventSink {
    /// Queue an event. Returns false if the receiver is gone.
    pub fn send(&self, mut event: ListingEvent) -> bool {
        // The order key of each name arriving, worked out here, on the
        // provider's thread, rather than on the window's as the listing
        // takes them (`Entry::name_key`).
        if let ListingEvent::Added(batch) = &mut event {
            for entry in batch {
                entry.name_key.0 = Some(crate::collate::sort_key(&entry.name));
            }
        }
        let ok = self.tx.send(event).is_ok();
        if let Some(w) = &self.waker {
            w();
        }
        ok
    }
}

/// An opened location. Dropping it (or `close`) stops all work for it.
///
/// The workspace asks `path_of`, `location_of`, `holds` and `close` while it holds
/// its lock, as panes are drawn, selected from and let go of: they answer
/// from what the location already knows, or cancel its work (handing
/// anything slower to a worker), and never take the workspace lock,
/// dispatch a command or wait. `hydrate` and `refresh`, which may start
/// work, are asked without it.
pub trait OpenLocation: Send + Sync {
    fn location(&self) -> &Location;

    /// The event stream. Never blocks the producer: the channel is unbounded.
    fn events(&self) -> &Receiver<ListingEvent>;

    /// Ask for metadata on these entries (lazy stat). Results arrive as `Updated`.
    fn hydrate(&self, ids: &[EntryId], priority: Priority);

    /// Where this entry lives on disk, if anywhere. Operations need this.
    fn path_of(&self, id: EntryId) -> Option<PathBuf>;

    /// Where a place (`EntryId::is_place`: an entry that is no file)
    /// leads: what opening it shows. Asked of a folder too, which leads
    /// to a location of the provider's when it is not the user's to open
    /// as a folder (a trashed folder stays in the trash, `trash:`); `None`
    /// opens a folder at its path.
    fn location_of(&self, _id: EntryId) -> Option<Location> {
        None
    }

    /// The folder it lists, and so holds open (its directory, its
    /// watches), when it lists one: a folder's own listing, and one a
    /// location lists through it (a trashed folder's). As the system
    /// resolved it once opened, a link on the way followed: what
    /// `App::release` compares with a volume's mount point, and lets go of
    /// before it is unmounted, whatever its scheme.
    fn holds(&self) -> Option<&Path> {
        None
    }

    /// Re-list from scratch (emits `Removed` for gone entries, `Added` for new).
    fn refresh(&self);

    /// Stop listing, stat, and watching.
    fn close(&self);
}

/// Something that can open locations of one scheme.
pub trait LocationProvider: Send + Sync {
    fn scheme(&self) -> &str;
    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>>;

    /// A name for `location` in words (a tab, a message), when the text
    /// after the scheme is not one: "PDF files in Projects" for a search.
    /// Answered from the text alone, without I/O: it is asked while
    /// drawing labels.
    fn title(&self, _location: &Location) -> Option<String> {
        None
    }

    /// `location` as a trail of places, for one that is no folder but
    /// lives in a hierarchy of its own (a tag in its categories): what the
    /// address draws, what Up goes back along. Answered from the text
    /// alone, like `title`. One that gives none reads as a single place,
    /// named as its tab is (`App::trail`): the trash, the shelf.
    fn trail(&self, _location: &Location) -> Option<Trail> {
        None
    }

    /// The bundled symbolic glyph that stands for `location` where it is
    /// named among others (its tab, the history) and in its pane when it
    /// lists nothing: `trash`, `tag`. A folder's when `None`. Answered
    /// from the text alone, like `title`.
    fn icon(&self, _location: &Location) -> Option<&'static str> {
        None
    }

    /// Whether `location` is a search: what it lists are results (counted
    /// so, "Searching…" while it walks), and its text is what the address
    /// edits (its pills, `query.describe`), words typed there editing it
    /// rather than filtering it. Answered from the text alone.
    fn searches(&self, _location: &Location) -> bool {
        false
    }

    /// The tags `location` says of every entry it lists, each of which has
    /// them or a tag within them: a tag's place, its tag (and those it is
    /// narrowed to: files that also have one); a search, the `tag:` terms
    /// every match meets. The views leave them out of each entry's tags,
    /// that tag exactly, never one within it: the place already says it.
    /// Answered from the text alone, like `title`.
    fn tags_shown(&self, _location: &Location) -> Vec<String> {
        Vec::new()
    }

    /// The folders opening `location` reads what is under, as its text
    /// resolves them: a search's root, or what the place it searches
    /// within reads. An agent's call opens it only when none is private
    /// or would reach another machine (`App::open_location`, ADR-044).
    /// Answered from the text alone, like `title`.
    fn folders_read(&self, _location: &Location) -> Vec<PathBuf> {
        Vec::new()
    }

    /// What a pane showing `location` says when it lists nothing, in the
    /// user's words: what the place holds and how it fills ("The shelf is
    /// empty"). Answered from the text alone, like `title`.
    fn empty_state(&self, _location: &Location) -> Option<EmptyState> {
        None
    }

    /// The places inside `location` (a tag's tags), each with where it
    /// leads: what the trail's menus list. Called on a worker.
    fn places(&self, _location: &Location) -> Vec<Crumb> {
        Vec::new()
    }

    /// The places of this scheme that are of `folder` (its tags,
    /// `tags:/?in=folder`), whose trail goes on from it (`Trail::base`):
    /// listed in the folder's menu of what it holds before its folders
    /// (`App::places_in`). `along` is where the trail shown goes on from
    /// `folder`: when it is one of these places as that trail has it
    /// (narrowed), give it as it is, so the menu marks it and choosing it
    /// keeps what narrows it. Answered from the locations alone, without
    /// I/O.
    fn places_of(&self, _folder: &Path, _along: Option<&Location>) -> Vec<Crumb> {
        Vec::new()
    }

    /// What gestures mean in `location`, when its entries are not its own
    /// files (ADR-032). Answered from the text alone, like `title`.
    fn verbs(&self, _location: &Location) -> Verbs {
        Verbs::default()
    }

    /// What `rest`, the text after the scheme being typed in the address
    /// field, can go on with (ADR-070): the name being typed, and every
    /// place it can be at that depth (a tag's tags, for `tags:/Places/Fr`).
    /// The address matches them as it matches a folder's names. `None` for
    /// a scheme whose text is not typed a name at a time (a search).
    /// Called on a worker.
    fn complete(&self, _rest: &str) -> Option<Completions> {
        None
    }
}

/// What a location's text being typed can go on with
/// (`LocationProvider::complete`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completions {
    /// The part of the text after the scheme being typed: what a
    /// completion's `text` replaces.
    pub range: std::ops::Range<usize>,
    /// That part as a name reads (its escapes undone): what the names are
    /// matched against.
    pub typed: String,
    pub items: Vec<Completion>,
}

/// A place a location's text being typed can go on with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// Its name as it reads: what is matched and shown.
    pub label: String,
    /// What replaces the part being typed, as the location's text writes
    /// it (escaped).
    pub text: String,
    /// Where it leads.
    pub location: Location,
    /// What follows it once taken when it holds places of its own, so the
    /// typing goes on inside it (`/`).
    pub inside: Option<char>,
}

/// One place of a trail: its words, and where it leads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Crumb {
    pub label: String,
    pub location: Location,
    /// The tag it stands for, by its full name as `keys::PLACE_TAG` names
    /// a place's: the address draws a place as that tag's chip, in its
    /// color, so it never reads as a folder, and a pill in that color,
    /// hollow for the files without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<crate::metadata::TagRef>,
}

impl Crumb {
    pub fn new(label: impl Into<String>, location: Location) -> Self {
        Crumb {
            label: label.into(),
            location,
            tag: None,
        }
    }

    /// The crumb, standing for `tag`.
    #[must_use]
    pub fn of_tag(mut self, tag: crate::metadata::TagRef) -> Self {
        self.tag = Some(tag);
        self
    }
}

/// A location that is no folder, read as a trail of places
/// (`LocationProvider::trail`): a tag location reads `Home › Tags ›
/// Places › France`, then what narrows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trail {
    /// The folder it is of, drawn first as that folder's own trail: the
    /// folder whose tags a tag location shows. What Up goes to from the
    /// first crumb.
    pub base: Option<PathBuf>,
    /// The places it is in, outermost first; the last is itself.
    pub crumbs: Vec<Crumb>,
    /// What narrows it ("with family"), each with where it leads without
    /// it: a pill after the crumbs, whose × goes there. Kept while going
    /// up and down the crumbs.
    pub filters: Vec<Crumb>,
    /// How it can show otherwise, for the View menu ("Include Sub-Tags").
    pub toggles: Vec<Toggle>,
    /// The place shown holds places of its own (a tag its tags, the
    /// trash its trashed folders, which `LocationProvider::places`
    /// lists): the trail ends with the chevron that lists them. A place
    /// that holds only files (the shelf, Recent) has none.
    pub holds_places: bool,
    /// What it lists is in one folder (a trashed folder's files): the list
    /// has no Folder column, which says where files from many are.
    pub one_folder: bool,
}

/// What a location says when it lists nothing
/// (`LocationProvider::empty_state`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmptyState {
    /// What is so: "The trash is empty".
    pub title: String,
    /// What fills it, or the way out: "Files moved to the trash wait
    /// here…".
    pub detail: String,
}

impl EmptyState {
    pub fn new(title: impl Into<String>, detail: impl Into<String>) -> Self {
        EmptyState {
            title: title.into(),
            detail: detail.into(),
        }
    }
}

/// A way a location can show otherwise, for the View menu: its words,
/// whether it does now, and where it leads switched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toggle {
    pub label: String,
    pub on: bool,
    pub location: Location,
}

impl Trail {
    /// Where Up goes from the location: the crumb before the last, else
    /// the folder it is of.
    pub fn parent(&self) -> Option<Location> {
        match self.crumbs.len() {
            0 | 1 => self.base.as_ref().map(Location::from_path),
            n => Some(self.crumbs[n - 2].location.clone()),
        }
    }
}

/// A gesture's words and command (`Verbs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Verb {
    /// What a menu item says: "Remove from “France”".
    pub title: String,
    pub command: crate::command::Invocation,
}

impl Verb {
    pub fn new(title: impl Into<String>, command: crate::command::Invocation) -> Self {
        Verb {
            title: title.into(),
            command,
        }
    }
}

/// What gestures mean in a location whose entries are not its own files,
/// but files kept elsewhere it lists (a tag's, the shelf's) and places
/// (ADR-032). Each `None` keeps what it means in a folder; a gesture with
/// no meaning in such a location does nothing there. The commands it
/// gives say what they act on: `pane` (that pane's selection) is added
/// when they run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verbs {
    /// Files dropped or pasted here: run with `pane` (the pane they are
    /// dragged from: its selection, and where they are) or `entries`
    /// (another application's files, or those of the clipboard with
    /// `from`, where they were cut or copied, and `places`, the places
    /// it holds), and `move` when they were cut or dragged without Ctrl.
    /// A tag's gives them its tag, and moved from another tag takes that
    /// one off; tags dropped go into it.
    pub drop: Option<crate::command::Invocation>,
    /// Taking the selection out: the Delete key there (`take_out`), and a
    /// menu item of these words. The files stay where they are: a tag's
    /// takes its tag off them, the shelf's unshelves them. Move to Trash
    /// keeps its meaning beside it.
    pub remove: Option<Verb>,
    /// Making a new place here: New Folder's key and menu item ("New
    /// Tag…").
    pub create: Option<Verb>,
    /// Renaming the place this location is, for its entry where it is
    /// listed (F2): run with the new name as `to`.
    pub rename: Option<Verb>,
    /// Narrowing it, for the Narrow by bar.
    pub narrow: Option<Narrowing>,
    /// Copying the place this location is into a folder, for its entry
    /// dropped or pasted there: run with `dest`. A tag's makes a folder of
    /// its files there.
    pub export: Option<crate::command::Invocation>,
    /// Its files are the trash's, put back where they came from by this
    /// (run with `pane`): a menu item for them. Move to Trash does not
    /// apply to them, and Delete deletes them for good, after asking. It
    /// keeps files, not copies of them: files copied are refused here
    /// (pasted after Copy, the split's Copy), as GNOME's Files refuses
    /// them, and a drag puts them in the trash whatever keys are held.
    pub restore: Option<crate::command::Invocation>,
    /// Buttons of its own in its pane's row, after the view's: the shelf's
    /// Move and Copy to the other pane, the trash's Empty Trash.
    pub tools: Vec<Tool>,
}

/// A button a location puts in its pane's row (`Verbs::tools`). It acts
/// on what the location lists, so it can be pressed only while that is
/// something, and where its `pane` says it can.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    /// The bundled symbolic glyph it shows (as `LocationProvider::icon`).
    pub icon: &'static str,
    /// What it does, its tooltip: "Empty Trash…".
    pub title: String,
    /// Run with `pane` added (`ToolPane`): a command that takes it, as
    /// commands refuse an argument they do not take.
    pub command: crate::command::Invocation,
    /// The pane it is run with, as `pane`.
    pub pane: ToolPane,
}

/// The pane a `Tool` is run with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPane {
    /// The pane showing the location.
    Shown,
    /// The other pane of the split, a folder its files go to: pressed only
    /// while the window is split and that pane shows a folder, so never
    /// when both show the shelf.
    OtherFolder,
}

impl Tool {
    pub fn new(
        icon: &'static str,
        title: impl Into<String>,
        command: crate::command::Invocation,
        pane: ToolPane,
    ) -> Self {
        Tool {
            icon,
            title: title.into(),
            command,
            pane,
        }
    }
}

/// What narrowing a location runs (the Narrow by bar's choices), and what
/// it narrows by already, which is not offered again.
#[derive(Clone, Debug, PartialEq)]
pub struct Narrowing {
    /// Run with `facet` and `value`: `also_tag` (files that also have the
    /// tag `value`), `not_tag` (none that have it), `kind` (of that kind,
    /// by its name), `modified` (modified that year).
    pub command: crate::command::Invocation,
    /// The tags every file listed has.
    pub tags: Vec<String>,
    /// The facets it narrows by already (`kind`, `modified`).
    pub set: Vec<String>,
}

/// What to sort a listing by. Directories always come first.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    /// An entry not stat-ed yet sorts after those that are, in either
    /// direction, as for every key below: a pane sorted by one asks for
    /// every entry's metadata (`Pane::unknown`).
    Size,
    Modified,
    /// The user's rating (`keys::RATING`); unrated sorts as none.
    Rating,
    /// When a file of the trash was put there (`keys::TRASHED`); a file
    /// that does not say sorts as the oldest.
    Deleted,
}

impl SortKey {
    /// Whether the order depends on what a stat finds, which a listing
    /// learns only when its entries are hydrated.
    pub fn needs_metadata(self) -> bool {
        matches!(self, SortKey::Size | SortKey::Modified | SortKey::Rating)
    }
}

/// A pane's sort and filter settings; the sorted order is cached in the listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SortSpec {
    pub key: SortKey,
    pub ascending: bool,
    /// Include dot-prefixed entries.
    pub show_hidden: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            key: SortKey::Name,
            ascending: true,
            show_hidden: false,
        }
    }
}

#[derive(Debug)]
struct SortCache {
    /// The version the order depends on whole: see [`Listing::sorted`].
    stamp: u64,
    spec: SortSpec,
    ids: Arc<[EntryId]>,
    /// Whether companions were shown when `ids` was made.
    companions_shown: bool,
    /// How many entries the listing held when `ids` was made: the ones
    /// after, in insertion order, are arrivals it has not placed yet.
    seen: usize,
    /// `ids`' sort keys in brief, side by side, so that arrivals and
    /// entries that moved find their place without looking every entry
    /// up (`Listing::place`).
    ranks: Vec<Rank>,
    /// Entries placed whose key under `spec` changed since (a size, a
    /// date, a rating): taken out and placed again.
    moved: Vec<EntryId>,
    /// Entries placed that the listing let go of since.
    gone: IdSet,
}

impl SortCache {
    /// Whether a change of `delta` moves an entry in this order.
    fn moves(&self, delta: &Delta) -> bool {
        match self.spec.key {
            SortKey::Name => false,
            SortKey::Size => delta.size,
            SortKey::Modified | SortKey::Deleted => delta.modified,
            SortKey::Rating => delta.rating,
        }
    }
}

/// An entry's sort key in brief: what `Listing::compare` compares, with
/// the name's collation key cut to its first sixteen bytes (eight tied a
/// quarter of the time: a run of digits takes two bytes of marker). Two ranks that
/// differ order their entries as the full keys do; equal ones are compared
/// in full.
#[derive(Clone, Copy, Debug)]
struct Rank {
    file: bool,
    unknown: bool,
    num: u64,
    prefix: u128,
    id: EntryId,
}

impl Rank {
    fn of(key: SortKeyOf<'_>) -> Self {
        let mut prefix = [0u8; 16];
        let n = key.3.len().min(16);
        prefix[..n].copy_from_slice(&key.3[..n]);
        Rank {
            file: key.0,
            unknown: key.1,
            num: key.2,
            prefix: u128::from_be_bytes(prefix),
            id: key.4,
        }
    }
}

#[derive(Debug)]
struct Slot {
    entry: Entry,
    /// [`crate::collate::sort_key`] of the name, computed once per name.
    key: Box<[u8]>,
    /// The file it is a companion of (ADR-028), by its name, when it is
    /// one; the empty name for a folder of companions (`.ts`).
    beside: Option<OsString>,
}

impl Slot {
    fn new(mut entry: Entry, companions: Option<&crate::companions::Companions>) -> Self {
        let key = entry
            .name_key
            .0
            .take()
            .unwrap_or_else(|| crate::collate::sort_key(&entry.name));
        let beside = companions.and_then(|c| {
            if c.is_folder(&entry.name) {
                Some(OsString::new())
            } else {
                c.owner(&entry.name)
            }
        });
        Slot { entry, key, beside }
    }
}

/// What one replacement changed, for the versions that depend on it.
// One flag per version it bumps; a state machine would say less.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct Delta {
    any: bool,
    name: bool,
    /// Something already listed may have moved. An arrival alone does not
    /// move anything: it is placed among the rest (`Listing::sorted`).
    order: bool,
    size: bool,
    /// A date changed: when it was modified, or put in the trash.
    modified: bool,
    /// A modification time already known changed: the file was written
    /// again, not stat-ed for the first time.
    rewritten: bool,
    rating: bool,
}

impl Delta {
    fn between(old: &Entry, new: &Entry) -> Self {
        use crate::entry::keys;
        let name = old.name != new.name;
        let size = old.metadata.value(keys::SIZE) != new.metadata.value(keys::SIZE)
            || old.metadata.value(keys::FILES) != new.metadata.value(keys::FILES);
        let modified = old.metadata.value(keys::MODIFIED) != new.metadata.value(keys::MODIFIED)
            || old.metadata.value(keys::TRASHED) != new.metadata.value(keys::TRASHED);
        let rewritten = old.metadata.value(keys::MODIFIED).is_some()
            && old.metadata.value(keys::MODIFIED) != new.metadata.value(keys::MODIFIED);
        let rating = old.metadata.value(keys::RATING) != new.metadata.value(keys::RATING);
        Delta {
            any: old != new,
            name,
            // Folders sort first, and a link sorts as what it leads to. A
            // file the system hides now, or no longer (a Mac's UF_HIDDEN
            // found by its stat, `attrib +h`), is shown or not: the order
            // is made again, as `shows` says.
            order: name || old.is_dir() != new.is_dir() || old.hidden != new.hidden,
            size,
            modified,
            rewritten,
            rating,
        }
    }

    /// Entries that went: nothing left moves (the order takes them out,
    /// `SortCache::gone`), unless one decided whether another is shown.
    fn removal(companions: bool) -> Self {
        Delta {
            any: true,
            name: true,
            order: companions,
            ..Delta::default()
        }
    }
}

/// How many entries `Listing::place` puts in their places one by one;
/// more are merged with the order in one pass.
const FEW_PLACED: usize = 8;

/// The renames a listing keeps for its holders until one asks
/// (`Listing::take_renamed`).
const RENAMED_KEPT: usize = 4096;

/// A set of entries asked about once per entry of a big order
/// (`Listing::place`): hashed by a multiply, an id being two numbers the
/// system gave, not a key anyone chooses.
type IdSet = std::collections::HashSet<EntryId, std::hash::BuildHasherDefault<IdHasher>>;

#[derive(Default)]
struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// What an entry sorts by: see `Listing::sort_key`.
type SortKeyOf<'a> = (bool, bool, u64, &'a [u8], EntryId);

/// Every version of every listing is read from this one clock, so a value
/// names one state of one listing. A pane that reopens its folder gets a
/// new listing whose versions cannot repeat the old one's: work keyed on a
/// version (a sorted order, requests already sent, measured names) sees the
/// replacement even though the location reads the same.
fn stamp() -> u64 {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// An in-memory view of a listing, kept up to date by applying events.
/// Insertion order is preserved so streaming order is stable; `sorted`
/// gives a cached order for a [`SortSpec`].
#[derive(Debug)]
pub struct Listing {
    /// Which listing this is; see [`stamp`].
    id: u64,
    entries: crate::hashing::IndexMap<EntryId, Slot>,
    complete: bool,
    error: Option<String>,
    /// What the answer rests on, when not the files as they are
    /// (`ListingEvent::AsOf`).
    as_of: Option<AsOf>,
    /// Incremented on every change, so views know when to recompute. An
    /// event that changes nothing (a stat that found what the listing
    /// already had) leaves it alone, so work keyed on it cannot loop.
    version: u64,
    /// Incremented when a name appears, disappears or changes. Hydration
    /// leaves it alone, so work that depends only on names (a grid counting
    /// the lines each one wraps to) is not redone for every metadata batch.
    names_version: u64,
    /// Incremented when a change can move an entry already listed under
    /// every sort: a removal, a new name or kind. Arrivals leave it alone:
    /// they are appended to `entries`, and `sorted` places them.
    order_version: u64,
    /// Incremented when a size or a date (modified, put in the trash)
    /// changes: only the sorts by those keys depend on them.
    size_version: u64,
    modified_version: u64,
    /// Incremented when a known modification time changes (`Delta::rewritten`).
    rewritten_version: u64,
    rating_version: u64,
    /// Incremented when the location itself changed (`ListingEvent::Changed`).
    itself_version: u64,
    /// Incremented when entries were taken out: one that comes back (a
    /// file given a tag again) arrives bare, to be hydrated anew.
    removed_version: u64,
    sort_cache: parking_lot::Mutex<Option<SortCache>>,
    /// The rules of files beside a file, to hide them (ADR-028).
    companions: Option<Arc<crate::companions::Companions>>,
    /// The names companions listed belong to: one arriving re-sorts, so
    /// its companion is hidden.
    owners: crate::hashing::Set<OsString>,
    /// Entries renamed since a holder last asked (`take_renamed`): each
    /// id before and after.
    renamed: Vec<(EntryId, EntryId)>,
    /// Where the location said it is reached (`ListingEvent::Redirect`),
    /// until its holder shows it (`take_redirect`).
    redirect: Option<Location>,
    /// Where the folder it lists is reached since it moved
    /// (`ListingEvent::Moved`), `None` while it is where it was opened.
    moved: Option<Location>,
    /// A move its holder has not followed yet (`take_moved`): where it
    /// was reached before (`None`: where it was opened), and now.
    unfollowed: Option<(Option<Location>, Location)>,
}

impl Default for Listing {
    fn default() -> Self {
        let id = stamp();
        Listing {
            id,
            entries: crate::hashing::IndexMap::default(),
            complete: false,
            error: None,
            as_of: None,
            version: id,
            names_version: id,
            order_version: id,
            size_version: id,
            modified_version: id,
            rewritten_version: id,
            rating_version: id,
            itself_version: id,
            removed_version: id,
            sort_cache: parking_lot::Mutex::new(None),
            companions: None,
            owners: crate::hashing::Set::default(),
            renamed: Vec::new(),
            redirect: None,
            moved: None,
            unfollowed: None,
        }
    }
}

/// A listing let go of on the window's thread (a pane showing another
/// folder, a tab going to sleep, a column closed) is freed on a worker.
impl Drop for Listing {
    fn drop(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.entries);
        let order = self.sort_cache.get_mut().take();
        let owners = std::mem::take(&mut self.owners);
        crate::scheduler::let_go((entries, order, owners));
    }
}

impl Listing {
    /// A listing that hides the files beside a file `companions` names,
    /// while their file is listed (ADR-028).
    pub fn with_companions(companions: Arc<crate::companions::Companions>) -> Self {
        let mut listing = Listing::default();
        listing.companions = Some(companions);
        listing
    }

    fn slot(&mut self, e: Entry) -> Slot {
        let slot = Slot::new(e, self.companions.as_deref());
        if let Some(owner) = &slot.beside {
            self.owners.insert(owner.clone());
        }
        slot
    }

    /// Whether companions are shown changed (`core.show_companions`):
    /// what is shown may, so the order is made again and whoever keys on
    /// the listing's versions sees it.
    pub fn companions_changed(&mut self) {
        if self.owners.is_empty() {
            return;
        }
        self.order_version = stamp();
        self.version = stamp();
    }

    /// Whether companions are shown now (with hidden files).
    fn companions_shown(&self) -> bool {
        self.companions.as_ref().is_some_and(|c| c.shown())
    }

    /// Apply one event. Returns whether the listing changed.
    pub fn apply(&mut self, event: ListingEvent) -> bool {
        let mut delta = Delta::default();
        match event {
            ListingEvent::Added(batch) => {
                for e in batch {
                    self.upsert(e, true, &mut delta);
                }
            }
            ListingEvent::Updated(batch) => {
                for e in batch {
                    self.upsert(e, false, &mut delta);
                }
            }
            ListingEvent::Removed(ids) => {
                let before = self.entries.len();
                // A companion gone, or the file one belonged to: whether
                // the other is shown changed, so the order is made again.
                let companions = !self.owners.is_empty()
                    && ids.iter().any(|id| {
                        self.entries.get(id).is_some_and(|slot| {
                            slot.beside.is_some() || self.owners.contains(&slot.entry.name)
                        })
                    });
                if let Some(c) = self.sort_cache.get_mut().as_mut() {
                    // What the order placed leaves it; what it had not
                    // placed yet leaves the arrivals still to place. The
                    // indexes are those before any of the batch goes, so
                    // they are told against what was placed then.
                    let placed = c.seen;
                    for id in &ids {
                        if let Some(at) = self.entries.get_index_of(id)
                            && at < placed
                            && c.gone.insert(*id)
                        {
                            c.seen -= 1;
                        }
                    }
                }
                if ids.len() > 1 {
                    // One pass instead of an O(n) shift per id: one id is
                    // one shift, as long as the pass.
                    let gone: crate::hashing::Set<EntryId> = ids.into_iter().collect();
                    self.entries.retain(|id, _| !gone.contains(id));
                } else {
                    for id in ids {
                        self.entries.shift_remove(&id);
                    }
                }
                if self.entries.len() != before {
                    delta = Delta::removal(companions);
                    self.removed_version = stamp();
                }
            }
            ListingEvent::Renamed(pairs) => {
                let (olds, news): (Vec<EntryId>, Vec<Entry>) = pairs.into_iter().unzip();
                let held: Vec<(EntryId, EntryId)> = olds
                    .iter()
                    .zip(&news)
                    .filter(|(old, new)| self.entries.contains_key(*old) && old != &&new.id)
                    .map(|(old, new)| (*old, new.id))
                    .collect();
                let gone: Vec<EntryId> = held.iter().map(|(old, _)| *old).collect();
                let removed = self.apply(ListingEvent::Removed(gone));
                let added = self.apply(ListingEvent::Added(news));
                self.renamed.extend(held);
                // A listing no holder asks (a test's, a tool's) keeps the
                // latest only.
                let over = self.renamed.len().saturating_sub(RENAMED_KEPT);
                self.renamed.drain(..over);
                return removed || added;
            }
            ListingEvent::Complete => {
                delta.any = !self.complete || self.error.is_some();
                self.complete = true;
                self.error = None;
            }
            ListingEvent::Error(e) => {
                delta.any = self.error.as_deref() != Some(e.as_str());
                self.error = Some(e);
            }
            ListingEvent::Changed => {
                self.itself_version = stamp();
                delta.any = true;
            }
            ListingEvent::AsOf(as_of) => {
                delta.any = self.as_of != as_of;
                self.as_of = as_of;
            }
            ListingEvent::Redirect(location) => {
                self.redirect = Some(location);
                delta.any = true;
            }
            ListingEvent::Moved(path) => {
                let now = Location::from_path(path);
                if self.moved.as_ref() != Some(&now) {
                    let before = match self.unfollowed.take() {
                        Some((before, _)) => before,
                        None => self.moved.clone(),
                    };
                    self.unfollowed = Some((before, now.clone()));
                    self.moved = Some(now);
                    delta.any = true;
                }
            }
        }
        for (moved, version) in [
            (delta.name, &mut self.names_version),
            (delta.order, &mut self.order_version),
            (delta.size, &mut self.size_version),
            (delta.modified, &mut self.modified_version),
            (delta.rewritten, &mut self.rewritten_version),
            (delta.rating, &mut self.rating_version),
            (delta.any, &mut self.version),
        ] {
            if moved {
                *version = stamp();
            }
        }
        delta.any
    }

    fn upsert(&mut self, mut e: Entry, insert: bool, delta: &mut Delta) {
        match self.entries.entry(e.id) {
            indexmap::map::Entry::Occupied(mut slot) => {
                let old = &slot.get().entry;
                // A re-listing (refresh, a watcher's rescan) sends bare
                // entries: keep what hydration found rather than blanking
                // every column until the rows are stat-ed again.
                if !e.metadata.is_hydrated() && old.metadata.is_hydrated() && old.name == e.name {
                    e.metadata = old.metadata.clone();
                    e.kind = old.kind;
                    e.target.clone_from(&old.target);
                    // What its stat said hides it, which a listing may not
                    // tell (a Mac's UF_HIDDEN): it is stat-ed again after.
                    e.hidden = old.hidden;
                }
                let d = Delta::between(old, &e);
                if !d.any {
                    return;
                }
                if let Some(c) = self.sort_cache.get_mut().as_mut()
                    && c.moves(&d)
                {
                    c.moved.push(e.id);
                }
                if d.name {
                    let replaced = Slot::new(e, self.companions.as_deref());
                    if let Some(owner) = &replaced.beside {
                        self.owners.insert(owner.clone());
                        delta.order = true;
                    }
                    if self.owners.contains(&replaced.entry.name) {
                        delta.order = true;
                    }
                    *slot.get_mut() = replaced;
                } else {
                    // Its slot keeps the key it has.
                    e.name_key.0 = None;
                    slot.get_mut().entry = e;
                }
                delta.any = true;
                delta.name |= d.name;
                delta.order |= d.order;
                delta.size |= d.size;
                delta.modified |= d.modified;
                delta.rewritten |= d.rewritten;
                delta.rating |= d.rating;
            }
            indexmap::map::Entry::Vacant(_) => {
                if insert {
                    let new = self.slot(e);
                    // A companion, or the file one belongs to: whether a
                    // listed entry is shown changed, so the order is made
                    // again (rare: a Mac's `._` files, sidecars).
                    if new.beside.is_some() || self.owners.contains(&new.entry.name) {
                        delta.order = true;
                    }
                    self.entries.insert(new.entry.id, new);
                    // A new name; nothing listed moved (see `order`).
                    delta.any = true;
                    delta.name = true;
                }
            }
        }
    }

    /// Changes so far; equal versions mean identical contents.
    /// Which listing this is: a pane that reopens its location, even the
    /// same one, holds a listing with another id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Changes, unique across listings: see [`stamp`].
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes to the set of names so far; see the field.
    pub fn names_version(&self) -> u64 {
        self.names_version
    }

    /// Changes to modification times so far, hydration's first ones
    /// included: work that depends on files' contents (where a picture was
    /// taken) looks again when it moves.
    pub fn modified_version(&self) -> u64 {
        self.modified_version
    }

    /// Files written again so far: a modification time known before that
    /// changed. Hydration learning one for the first time leaves it alone,
    /// so work that reads files' contents (where a picture was taken)
    /// looks again when a file changes, not when a row is first stat-ed.
    pub fn rewritten_version(&self) -> u64 {
        self.rewritten_version
    }

    /// Changes to sizes (and to the files a place counts) so far,
    /// hydration's first ones included.
    pub fn size_version(&self) -> u64 {
        self.size_version
    }

    /// Changes to the location itself (its folder's attributes, its tags)
    /// so far: what describes the folder shown reads it again when it moves.
    pub fn itself_version(&self) -> u64 {
        self.itself_version
    }

    /// Entries taken out so far: what remembers asking for an entry's
    /// metadata forgets those gone, which come back bare if they come back.
    pub fn removed_version(&self) -> u64 {
        self.removed_version
    }

    /// The ids in `spec` order, directories first. Cached until something
    /// the order depends on changes: a metadata batch moves only what it
    /// changed under a sort by size, date or rating, so calling this every
    /// frame is cheap. Entries that arrived, moved or went since are put
    /// in their places in the cached order (`place`) rather than sorting
    /// again: a folder of 366 000 entries streams in hundreds of batches,
    /// and hydrated in as many more under a sort by size, and sorting it
    /// whole after each one cost a tenth of a second a time.
    /// Until the listing is complete, the order may leave out the latest
    /// arrivals (see the placement rule below); once it is, it holds every
    /// shown entry.
    pub fn sorted(&self, spec: SortSpec) -> Arc<[EntryId]> {
        let companions_shown = self.companions_shown();
        let mut cache = self.sort_cache.lock();
        if let Some(c) = cache.as_mut()
            && c.stamp == self.order_version
            && c.spec == spec
            && c.companions_shown == companions_shown
        {
            // While the listing streams in, arrivals are placed once they
            // are a quarter of what is placed: each placement copies the
            // order, so placing every batch of a folder of 366 000 entries
            // copied it hundreds of times. Growing by quarters, the copies
            // add up to a few times the final order; the order shown lags by
            // at most that much, and is whole once the listing completes.
            // What went is never left in it.
            let waiting = self.entries.len() - c.seen + c.moved.len();
            if !c.gone.is_empty() || waiting > 0 && (self.complete || waiting * 4 >= c.ranks.len())
            {
                self.place(c, spec);
            }
            return Arc::clone(&c.ids);
        }
        let keyed = self.keyed(spec);
        let ids: Arc<[EntryId]> = keyed.iter().map(|key| key.4).collect();
        *cache = Some(SortCache {
            stamp: self.order_version,
            spec,
            companions_shown,
            ids: Arc::clone(&ids),
            seen: self.entries.len(),
            ranks: keyed.into_iter().map(Rank::of).collect(),
            moved: Vec::new(),
            gone: IdSet::default(),
        });
        ids
    }

    /// What an entry is sorted by under `spec`: files after directories,
    /// then whether its key is still unknown (not stat-ed yet: after the
    /// rest, whichever the direction), then the size or date (0 for a
    /// name sort, or when the entry has none), then the name's collation
    /// key, then the id, so that no two entries tie: the order is one
    /// order, whether made whole or by placing arrivals, and equal names do
    /// not trade places from one sort to the next.
    fn sort_key(spec: SortSpec, slot: &Slot) -> SortKeyOf<'_> {
        use crate::entry::{Value, keys};
        let e = &slot.entry;
        let unknown = spec.key.needs_metadata() && !e.metadata.is_hydrated();
        let num = match spec.key {
            SortKey::Name => 0,
            // A place by how many files it holds.
            SortKey::Size => e
                .metadata
                .value(if e.is_place() {
                    keys::FILES
                } else {
                    keys::SIZE
                })
                .and_then(Value::as_u64)
                .unwrap_or(0),
            SortKey::Modified | SortKey::Deleted => e
                .metadata
                .value(if spec.key == SortKey::Modified {
                    keys::MODIFIED
                } else {
                    keys::TRASHED
                })
                .and_then(Value::as_time)
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            SortKey::Rating => e
                .metadata
                .value(keys::RATING)
                .and_then(Value::as_u64)
                .unwrap_or(0),
        };
        (!e.is_dir(), unknown, num, &*slot.key, e.id)
    }

    fn compare(spec: SortSpec, a: SortKeyOf<'_>, b: SortKeyOf<'_>) -> std::cmp::Ordering {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| {
                let o = a.2.cmp(&b.2).then_with(|| a.3.cmp(b.3));
                if spec.ascending { o } else { o.reverse() }
            })
            .then_with(|| a.4.cmp(&b.4))
    }

    /// Whether `slot` is shown under `spec`: a hidden file only with hidden
    /// files; a companion whose file is listed (in `listed`) only with
    /// hidden files and companions both; one whose file is gone, an
    /// orphan, as any file.
    fn shown(
        spec: SortSpec,
        slot: &Slot,
        listed: Option<&crate::hashing::Set<&OsStr>>,
        companions: bool,
    ) -> bool {
        let with_file = match (&slot.beside, listed) {
            (Some(owner), _) if owner.is_empty() => true,
            (Some(owner), Some(listed)) => listed.contains(owner.as_os_str()),
            _ => false,
        };
        if with_file {
            return spec.show_hidden && companions;
        }
        spec.show_hidden || !slot.entry.is_hidden()
    }

    /// Whether an entry is listed and shown under `spec`: not a hidden file
    /// while hidden files are not shown, nor a companion hidden beside its
    /// file. The names a companion asks after are gathered once, and only
    /// when one is asked about.
    pub fn shows(&self, spec: SortSpec) -> impl Fn(EntryId) -> bool + '_ {
        let companions = self.companions_shown();
        let listed = std::cell::OnceCell::new();
        move |id| {
            self.entries.get(&id).is_some_and(|slot| {
                let listed = slot
                    .beside
                    .as_ref()
                    .and_then(|_| listed.get_or_init(|| self.listed()).as_ref());
                Self::shown(spec, slot, listed, companions)
            })
        }
    }

    /// The names listed, when companions may hide: their files among them.
    fn listed(&self) -> Option<crate::hashing::Set<&OsStr>> {
        (!self.owners.is_empty()).then(|| {
            self.entries
                .values()
                .map(|slot| slot.entry.name.as_os_str())
                .collect()
        })
    }

    /// The shown entries' sort keys, in order.
    fn keyed(&self, spec: SortSpec) -> Vec<SortKeyOf<'_>> {
        let listed = self.listed();
        let companions = self.companions_shown();
        let mut keyed: Vec<SortKeyOf<'_>> = self
            .entries
            .values()
            .filter(|slot| Self::shown(spec, slot, listed.as_ref(), companions))
            .map(|slot| Self::sort_key(spec, slot))
            .collect();
        keyed.sort_unstable_by(|a, b| Self::compare(spec, *a, *b));
        keyed
    }

    #[cfg(test)]
    fn compute_sorted(&self, spec: SortSpec) -> Arc<[EntryId]> {
        self.keyed(spec).iter().map(|key| key.4).collect()
    }

    /// Where `placed` (a rank of the cached order) stands against `key`,
    /// as `compare` says: by the ranks, and by the full keys when they tie.
    fn compare_rank(
        &self,
        spec: SortSpec,
        placed: &Rank,
        key: SortKeyOf<'_>,
        rank: &Rank,
    ) -> std::cmp::Ordering {
        placed
            .file
            .cmp(&rank.file)
            .then_with(|| placed.unknown.cmp(&rank.unknown))
            .then_with(|| {
                let o = (placed.num, placed.prefix)
                    .cmp(&(rank.num, rank.prefix))
                    .then_with(|| {
                        let slot = self.entries.get(&placed.id).expect("a ranked id is listed");
                        (*slot.key).cmp(key.3)
                    });
                if spec.ascending { o } else { o.reverse() }
            })
            .then_with(|| placed.id.cmp(&rank.id))
    }

    /// The cached order with what changed put in its place: the entries
    /// that went taken out, those that moved taken out and placed again
    /// with the arrivals (the entries after its first `seen`), which are
    /// sorted among themselves and merged with the ranks in one pass, which
    /// the copy of the order costs anyway. A new name or kind never comes
    /// here; it changes `order_version`, and the order is made again.
    fn place(&self, cache: &mut SortCache, spec: SortSpec) {
        let listed = self.listed();
        let companions = self.companions_shown();
        let shown = |slot: &&Slot| Self::shown(spec, slot, listed.as_ref(), companions);
        let mut out = std::mem::take(&mut cache.gone);
        let mut again: Vec<SortKeyOf<'_>> = Vec::new();
        for id in std::mem::take(&mut cache.moved) {
            // One not placed yet is an arrival, placed below.
            if let Some((at, _, slot)) = self.entries.get_full(&id)
                && at < cache.seen
                && out.insert(id)
                && shown(&slot)
            {
                again.push(Self::sort_key(spec, slot));
            }
        }
        again.extend(
            self.entries.as_slice()[cache.seen..]
                .values()
                .filter(shown)
                .map(|slot| Self::sort_key(spec, slot)),
        );
        again.sort_unstable_by(|a, b| Self::compare(spec, *a, *b));
        // A few (a file growing, one arriving): each found and put in its
        // place where the order is, a search apiece instead of a merge
        // that compares every rank.
        if out.len() + again.len() <= FEW_PLACED {
            let ranks = &mut cache.ranks;
            ranks.retain(|r| !out.contains(&r.id));
            for key in again {
                let rank = Rank::of(key);
                let at = ranks.partition_point(|p| {
                    self.compare_rank(spec, p, key, &rank) != std::cmp::Ordering::Greater
                });
                ranks.insert(at, rank);
            }
            cache.ids = ranks.iter().map(|r| r.id).collect();
            cache.seen = self.entries.len();
            return;
        }
        let placed = &cache.ranks;
        let mut ranks = Vec::with_capacity(placed.len() + again.len());
        let mut old = placed
            .iter()
            .filter(|r| out.is_empty() || !out.contains(&r.id))
            .peekable();
        for key in again {
            let rank = Rank::of(key);
            while let Some(p) = old
                .next_if(|p| self.compare_rank(spec, p, key, &rank) != std::cmp::Ordering::Greater)
            {
                ranks.push(*p);
            }
            ranks.push(rank);
        }
        ranks.extend(old.copied());
        let ids: Vec<EntryId> = ranks.iter().map(|r| r.id).collect();
        cache.ids = ids.into();
        cache.ranks = ranks;
        cache.seen = self.entries.len();
    }

    /// Apply what was queued when called, until `budget` has passed, without
    /// blocking; at least one event. Returns how many events changed the
    /// listing. What is left waits for the next call: a location still
    /// streaming never keeps the caller (the UI thread) here, and a backlog
    /// is applied a slice at a time, with frames drawn in between.
    pub fn drain(&mut self, rx: &Receiver<ListingEvent>, budget: std::time::Duration) -> usize {
        let started = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..rx.len() {
            let Ok(ev) = rx.try_recv() else { break };
            n += usize::from(self.apply(ev));
            if started.elapsed() >= budget {
                break;
            }
        }
        n
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// What the answer rests on when it is an index's as of its last
    /// update, not the files as they are now.
    pub fn as_of(&self) -> Option<&AsOf> {
        self.as_of.as_ref()
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(&id).map(|s| &s.entry)
    }

    pub fn get_index(&self, i: usize) -> Option<&Entry> {
        self.entries.get_index(i).map(|(_, s)| &s.entry)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().map(|s| &s.entry)
    }

    pub fn ids(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.entries.keys().copied()
    }

    pub fn find_by_name(&self, name: &OsStr) -> Option<&Entry> {
        self.iter().find(|e| e.name == name)
    }

    /// The entries renamed since this was last asked, each id before and
    /// after (`ListingEvent::Renamed`): what keeps them by id (a
    /// selection) follows them.
    pub fn take_renamed(&mut self) -> Vec<(EntryId, EntryId)> {
        std::mem::take(&mut self.renamed)
    }

    /// Where the location said it is reached, once (`ListingEvent::Redirect`).
    pub fn take_redirect(&mut self) -> Option<Location> {
        self.redirect.take()
    }

    /// Where the folder it lists is reached since it moved
    /// (`ListingEvent::Moved`): the address a path location shows from
    /// then on ([`Listing::address`]).
    pub fn moved_to(&self) -> Option<&Location> {
        self.moved.as_ref()
    }

    /// The address `open`, listed by this listing, is reached at: where
    /// its folder moved for a path location, else where it was opened. A
    /// location of another scheme that lists a folder through a path's
    /// listing (a trashed folder's) keeps its own.
    pub fn address<'a>(&'a self, open: &'a dyn OpenLocation) -> &'a Location {
        let opened = open.location();
        match &self.moved {
            Some(moved) if opened.is_path() => moved,
            _ => opened,
        }
    }

    /// The folder it lists is now at `now`, moved with a folder above it,
    /// which its holder followed: no event says so, a watch seeing only
    /// the folder it watches move.
    pub(crate) fn relocate(&mut self, now: Location) {
        self.moved = Some(now);
    }

    /// The move its holder has not followed yet, once: where the folder
    /// was reached before (`None`: where it was opened) and now.
    pub(crate) fn take_moved(&mut self) -> Option<(Option<Location>, Location)> {
        self.unfollowed.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;

    /// What `escape` writes reads back; a `%` not followed by two hex
    /// digits (a sign among them) is itself.
    #[test]
    fn an_escape_is_a_percent_and_two_hex_digits() {
        let name = "l'été 100% #1?\u{7}".as_bytes();
        assert_eq!(unescape(&escape(name)), name);
        assert_eq!(unescape("%41%e9%E9"), [b'A', 0xe9, 0xe9]);
        for kept in ["%+1", "%-1", "% 1", "%4", "%", "%zz"] {
            assert_eq!(unescape(kept), kept.as_bytes(), "{kept}");
        }
    }

    #[test]
    fn what_an_answer_rests_on_is_kept_until_a_run_says_otherwise() {
        let mut listing = Listing::default();
        assert_eq!(listing.as_of(), None, "the files as they are");
        let as_of = AsOf {
            by: "an index".into(),
            updated: Some(std::time::SystemTime::UNIX_EPOCH),
        };
        let before = listing.version();
        assert!(listing.apply(ListingEvent::AsOf(Some(as_of.clone()))));
        assert_eq!(listing.as_of(), Some(&as_of));
        assert_ne!(listing.version(), before, "who draws it sees it");
        let said = listing.version();
        assert!(!listing.apply(ListingEvent::AsOf(Some(as_of.clone()))));
        assert_eq!(listing.version(), said, "said again, nothing changed");
        assert!(listing.apply(ListingEvent::Complete));
        assert_eq!(listing.as_of(), Some(&as_of), "complete keeps it");
        assert!(
            listing.apply(ListingEvent::AsOf(None)),
            "a walk takes it back"
        );
        assert_eq!(listing.as_of(), None);
    }

    #[test]
    fn a_folder_is_the_same_place_by_its_path_anything_else_as_written() {
        let home = Location::parse("~").path().unwrap();
        assert!(Location::same_place(
            &Location::parse("~/a b"),
            &Location::from_path(home.join("a b"))
        ));
        assert!(!Location::same_place(
            &Location::from_path("/a"),
            &Location::from_path("/b")
        ));
        assert!(Location::same_place(
            &Location::parse("query:ext:pdf in:/x"),
            &Location::new("query", "ext:pdf in:/x")
        ));
        assert!(!Location::same_place(
            &Location::parse("query:ext:pdf"),
            &Location::parse("query:ext:png")
        ));
        assert!(!Location::same_place(
            &Location::parse("/query"),
            &Location::parse("query:/")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_location_argument_reads_back_as_the_location() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(OsStr::from_bytes(b"/x/caf\xe9"));
        for location in [
            Location::new("tags", "/Places"),
            Location::from_path("/x/y"),
            Location::from_path(odd),
        ] {
            let arg = location.arg();
            assert_eq!(Location::parse_os(arg.as_os_str()), location);
            let json = serde_json::to_value(&arg).unwrap();
            let back: PathArg = serde_json::from_value(json).unwrap();
            assert_eq!(back, arg, "exact through JSON");
        }
        assert_eq!(
            Location::new("tags", "/Places").arg(),
            PathArg::from(Path::new("tags:/Places"))
        );
    }

    #[test]
    fn names_version_ignores_hydration() {
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            file(1, "a.txt"),
            file(2, "b.txt"),
        ]));
        let (names, version) = (listing.names_version(), listing.version());

        // The same entries again, as a metadata batch sends them.
        listing.apply(ListingEvent::Updated(vec![file(1, "a.txt")]));
        listing.apply(ListingEvent::Complete);
        assert_eq!(listing.names_version(), names);
        assert!(listing.version() > version);

        // A rename keeps the inode, so the id: only the name tells.
        listing.apply(ListingEvent::Updated(vec![file(1, "renamed.txt")]));
        let renamed = listing.names_version();
        assert!(renamed > names);
        listing.apply(ListingEvent::Removed(vec![EntryId {
            file: crate::FileId::new(1, 9),
            at: 0,
        }]));
        assert_eq!(listing.names_version(), renamed, "nothing was removed");
        listing.apply(ListingEvent::Removed(vec![EntryId {
            file: crate::FileId::new(1, 2),
            at: 0,
        }]));
        assert!(listing.names_version() > renamed);
    }

    #[test]
    fn a_reopened_location_never_repeats_a_version() {
        // A pane that reopens the folder it shows gets a fresh listing of the
        // same entries. Work keyed on the old one's versions (the hydration
        // it asked for, its sorted order) must see the change.
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut old = Listing::default();
        old.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        let mut new = Listing::default();
        assert_ne!(new.id(), old.id());
        new.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        for (a, b) in [
            (old.version(), new.version()),
            (old.names_version(), new.names_version()),
        ] {
            assert_ne!(a, b);
        }
        assert_ne!(Listing::default().version(), Listing::default().version());
    }

    fn stat(mut e: Entry, size: u64) -> Entry {
        use crate::entry::{Provenance, Value, keys};
        e.metadata
            .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        e
    }

    #[test]
    fn a_relisting_keeps_what_hydration_found() {
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 7)]));
        let version = listing.version();

        // A refresh sends the same entries bare: nothing changes.
        assert!(!listing.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")])));
        assert_eq!(listing.version(), version);
        assert!(
            listing
                .get(EntryId {
                    file: crate::FileId::new(1, 1),
                    at: 0
                })
                .unwrap()
                .metadata
                .is_hydrated()
        );

        // A stat that finds the same thing again changes nothing either, so
        // work keyed on the version cannot loop on it.
        assert!(!listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 7)])));
        assert_eq!(listing.version(), version);

        // An inode reused under another name is another file.
        listing.apply(ListingEvent::Added(vec![file(1, "c")]));
        assert!(
            !listing
                .get(EntryId {
                    file: crate::FileId::new(1, 1),
                    at: 0
                })
                .unwrap()
                .metadata
                .is_hydrated()
        );
    }

    /// A file the system comes to hide (a Mac's `UF_HIDDEN` its stat finds,
    /// `attrib +h`) leaves what is drawn as it leaves what is shown, and a
    /// re-listing that cannot tell keeps it hidden.
    #[test]
    fn a_file_hidden_while_listed_leaves_the_order() {
        let file = |ino, name: &str, hidden: bool| {
            let mut e = Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            );
            e.hidden = hidden;
            e
        };
        let spec = SortSpec::default();
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            file(1, "Library", false),
            file(2, "it's shown", false),
        ]));
        assert_eq!(listing.sorted(spec).len(), 2);
        let mut hidden = stat(file(1, "Library", true), 7);
        hidden.hidden = true;
        listing.apply(ListingEvent::Updated(vec![hidden.clone()]));
        assert_eq!(&*listing.sorted(spec), [file(2, "", false).id]);
        assert!(!listing.shows(spec)(hidden.id));
        // Listed again bare, as a listing says it (no UF_HIDDEN): still
        // hidden until stat-ed again.
        listing.apply(ListingEvent::Added(vec![file(1, "Library", false)]));
        assert_eq!(listing.sorted(spec).len(), 1);
        // No longer hidden: shown again.
        let mut shown = hidden;
        shown.hidden = false;
        listing.apply(ListingEvent::Updated(vec![shown]));
        assert_eq!(listing.sorted(spec).len(), 2);
    }

    #[test]
    fn a_late_update_does_not_resurrect_a_removed_entry() {
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![file(1, "a")]));
        listing.apply(ListingEvent::Removed(vec![EntryId {
            file: crate::FileId::new(1, 1),
            at: 0,
        }]));
        assert!(!listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 1)])));
        assert!(listing.is_empty());
    }

    #[test]
    fn hydration_re_sorts_only_the_sorts_that_depend_on_it() {
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            file(1, "f10"),
            file(2, "f2"),
            file(3, "É"),
            file(4, "z"),
        ]));
        let by_name = SortSpec::default();
        let by_size = SortSpec {
            key: SortKey::Size,
            ..SortSpec::default()
        };
        let names = listing.sorted(by_name);
        let ino: Vec<u64> = names.iter().map(|id| id.file.ino).collect();
        assert_eq!(ino, [3, 2, 1, 4], "natural and case-folded");
        listing.apply(ListingEvent::Updated(vec![stat(file(4, "z"), 1)]));
        assert!(
            Arc::ptr_eq(&names, &listing.sorted(by_name)),
            "not sorted again"
        );
        // What is not stat-ed yet comes after what is, by name.
        let sizes: Vec<u64> = listing
            .sorted(by_size)
            .iter()
            .map(|id| id.file.ino)
            .collect();
        assert_eq!(sizes, [4, 3, 2, 1]);
        listing.apply(ListingEvent::Updated(vec![stat(file(2, "f2"), 9)]));
        let sizes: Vec<u64> = listing
            .sorted(by_size)
            .iter()
            .map(|id| id.file.ino)
            .collect();
        assert_eq!(sizes, [4, 2, 3, 1]);
        let largest_first = SortSpec {
            ascending: false,
            ..by_size
        };
        let sizes: Vec<u64> = listing
            .sorted(largest_first)
            .iter()
            .map(|id| id.file.ino)
            .collect();
        assert_eq!(sizes, [2, 4, 1, 3], "and still after it the other way");
    }

    #[test]
    fn a_rating_changed_on_disk_sorts_the_listing_again() {
        use crate::entry::{Provenance, Value, keys};
        let rated = |ino, name: &str, rating: Option<u64>| {
            let mut e = stat(
                Entry::new(
                    EntryId {
                        file: crate::FileId::new(1, ino),
                        at: 0,
                    },
                    name,
                    EntryKind::File,
                ),
                1,
            );
            if let Some(r) = rating {
                e.metadata
                    .set(keys::RATING, Value::UInt(r), Provenance::User);
            }
            e
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            rated(1, "a", Some(4)),
            rated(2, "b", None),
            rated(3, "c", Some(10)),
        ]));
        let best_first = SortSpec {
            key: SortKey::Rating,
            ascending: false,
            ..SortSpec::default()
        };
        let order = |l: &Listing| {
            l.sorted(best_first)
                .iter()
                .map(|id| id.file.ino)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&listing), [3, 1, 2], "unrated last");
        listing.apply(ListingEvent::Updated(vec![rated(2, "b", Some(6))]));
        assert_eq!(order(&listing), [3, 2, 1]);
    }

    #[test]
    fn large_removals_keep_the_order_of_what_remains() {
        let file = |ino| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                format!("n{ino}"),
                EntryKind::File,
            )
        };
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added((0..100).map(file).collect()));
        listing.apply(ListingEvent::Removed(
            (0..100)
                .filter(|i| i % 3 != 0)
                .map(|ino| EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                })
                .collect(),
        ));
        let left: Vec<u64> = listing.ids().map(|id| id.file.ino).collect();
        assert_eq!(left, (0..100).filter(|i| i % 3 == 0).collect::<Vec<_>>());
    }

    #[cfg(unix)]
    #[test]
    fn path_locations_keep_names_that_are_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let raw = Path::new(OsStr::from_bytes(b"/example/caf\xe9"));
        let loc = Location::from_path(raw);
        assert_eq!(loc.path().as_deref(), Some(raw));
        let json = serde_json::to_value(&loc).unwrap();
        let back: Location = serde_json::from_value(json).unwrap();
        assert_eq!(back, loc);
        assert_eq!(Location::parse_os(raw.as_os_str()), loc);
        assert_eq!(
            Location::from_path("/example/a"),
            Location::parse("/example/a")
        );
    }

    /// Arrivals placed in the cached order give the order a full sort
    /// gives, under every sort, hidden or not, however the batches fall;
    /// a removal or a rename in between is sorted again, and the complete
    /// listing's order holds everything.
    #[test]
    fn arrivals_are_placed_where_a_full_sort_puts_them() {
        use crate::entry::{Provenance, Value, keys};
        let entry = |ino: u64| {
            // Names that repeat and differ in case, some hidden, some
            // folders, sizes that tie: the orders' tie-breaks are exercised.
            let name = match ino % 5 {
                0 => format!(".hidden-{}", ino % 7),
                1 => format!("Name {}", ino % 11),
                2 => format!("name {}", ino % 11),
                _ => format!("é{:03}", ino.wrapping_mul(37) % 101),
            };
            let kind = if ino.is_multiple_of(3) {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let mut e = Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                &name,
                kind,
            );
            e.metadata
                .set(keys::SIZE, Value::UInt(ino % 4), Provenance::Filesystem);
            e
        };
        for key in [SortKey::Name, SortKey::Size, SortKey::Modified] {
            for (ascending, show_hidden) in [(true, false), (false, true), (true, true)] {
                let spec = SortSpec {
                    key,
                    ascending,
                    show_hidden,
                };
                let mut listing = Listing::default();
                let mut next = 0;
                for batch in [1, 7, 0, 30, 2, 64, 5] {
                    let arrivals = (next..next + batch).map(entry).collect();
                    next += batch;
                    listing.apply(ListingEvent::Added(arrivals));
                    // While it streams, the order may leave arrivals out
                    // but never misplaces what it holds.
                    let shown = listing.sorted(spec);
                    let held: std::collections::HashSet<_> = shown.iter().collect();
                    let whole: Vec<_> = listing
                        .compute_sorted(spec)
                        .iter()
                        .filter(|id| held.contains(id))
                        .copied()
                        .collect();
                    assert_eq!(*shown, *whole, "{spec:?}");
                    if batch == 30 {
                        listing.apply(ListingEvent::Removed(vec![EntryId {
                            file: crate::FileId::new(1, 3),
                            at: 0,
                        }]));
                        listing.apply(ListingEvent::Updated(vec![Entry::new(
                            EntryId {
                                file: crate::FileId::new(1, 4),
                                at: 0,
                            },
                            "aaa renamed",
                            EntryKind::File,
                        )]));
                        assert_eq!(*listing.sorted(spec), *listing.compute_sorted(spec));
                    }
                }
                listing.apply(ListingEvent::Complete);
                assert_eq!(
                    *listing.sorted(spec),
                    *listing.compute_sorted(spec),
                    "{spec:?}"
                );
            }
        }
    }

    /// A listing hydrated in batches once complete, as a sort by size asks
    /// for every entry's metadata, keeps the order a full sort gives: what
    /// a batch stat-ed moves from among the unknown to its place, what
    /// went leaves, whichever the direction.
    #[test]
    fn hydration_and_removals_are_placed_where_a_full_sort_puts_them() {
        use crate::entry::{Provenance, Value, keys};
        let bare = |ino: u64| {
            let kind = if ino.is_multiple_of(7) {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                format!("n{}", ino % 13),
                kind,
            )
        };
        let stated = |ino: u64| {
            let mut e = bare(ino);
            e.metadata.set(
                keys::SIZE,
                Value::UInt(ino.wrapping_mul(2_654_435_761) % 9),
                Provenance::Filesystem,
            );
            e.metadata.set(
                keys::MODIFIED,
                Value::Time(std::time::UNIX_EPOCH + std::time::Duration::from_secs(ino % 5)),
                Provenance::Filesystem,
            );
            e
        };
        for key in [SortKey::Size, SortKey::Modified, SortKey::Name] {
            for ascending in [true, false] {
                let spec = SortSpec {
                    key,
                    ascending,
                    show_hidden: false,
                };
                let mut listing = Listing::default();
                listing.apply(ListingEvent::Added((0..200).map(bare).collect()));
                listing.apply(ListingEvent::Complete);
                assert_eq!(*listing.sorted(spec), *listing.compute_sorted(spec));
                for (n, batch) in (0..200).collect::<Vec<u64>>().chunks(23).enumerate() {
                    listing.apply(ListingEvent::Updated(
                        batch.iter().map(|i| stated(*i)).collect(),
                    ));
                    if n % 3 == 1 {
                        listing.apply(ListingEvent::Removed(vec![
                            EntryId {
                                file: crate::FileId::new(1, batch[0]),
                                at: 0,
                            },
                            EntryId {
                                file: crate::FileId::new(1, 199 - batch[1]),
                                at: 0,
                            },
                        ]));
                    }
                    if n == 4 {
                        listing.apply(ListingEvent::Added(vec![stated(500), bare(501)]));
                    }
                    assert_eq!(
                        *listing.sorted(spec),
                        *listing.compute_sorted(spec),
                        "{spec:?} after batch {n}"
                    );
                }
                // One at a time, as a watcher's changes come: put in place
                // by a search each.
                for ino in [10, 11, 600, 12] {
                    if ino == 600 {
                        listing.apply(ListingEvent::Added(vec![stated(ino)]));
                    } else {
                        let mut e = stated(ino);
                        e.metadata
                            .set(keys::SIZE, Value::UInt(ino % 4), Provenance::Filesystem);
                        listing.apply(ListingEvent::Updated(vec![e]));
                    }
                    assert_eq!(*listing.sorted(spec), *listing.compute_sorted(spec));
                }
                listing.apply(ListingEvent::Removed(vec![EntryId {
                    file: crate::FileId::new(1, 11),
                    at: 0,
                }]));
                assert_eq!(*listing.sorted(spec), *listing.compute_sorted(spec));
            }
        }
    }

    /// A location that keeps sending never keeps the drain: it applies
    /// what was queued when called, and leaves the rest for the next call.
    #[test]
    fn a_drain_leaves_what_arrives_during_it_for_the_next() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let file = |ino: u64| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                ino.to_string(),
                EntryKind::File,
            )
        };
        for ino in 0..3 {
            tx.send(ListingEvent::Added(vec![file(ino)])).unwrap();
        }
        let mut listing = Listing::default();
        let long = std::time::Duration::from_secs(60);
        assert_eq!(listing.drain(&rx, long), 3);
        tx.send(ListingEvent::Added(vec![file(9)])).unwrap();
        tx.send(ListingEvent::Added(vec![file(10)])).unwrap();
        // No time at all: one event, the other waits.
        assert_eq!(listing.drain(&rx, std::time::Duration::ZERO), 1);
        assert_eq!(rx.len(), 1);
        assert_eq!(listing.drain(&rx, long), 1);
        assert_eq!(listing.len(), 5);
    }

    /// What a provider may send, in any order, read between any two of its
    /// events: the order kept is the one sorting it whole gives once the
    /// listing is complete, holds only what is listed before, and never
    /// panics. Empty Trash found an order
    /// kept past the entries it held (2026-10-10).
    #[test]
    fn the_kept_order_follows_any_events() {
        use crate::entry::{Provenance, Value, keys};
        use proptest::prelude::*;

        #[derive(Clone, Debug)]
        enum Event {
            Add(Vec<u64>),
            Grow(u64, u64),
            Remove(Vec<u64>),
            Rename(u64, u64),
            Complete,
            Sort(bool),
        }
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let entry = |ino: u64, size: u64| {
            let mut e = Entry::new(id(ino), format!("n{:02}", (ino * 7) % 23), EntryKind::File);
            e.metadata
                .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
            e
        };
        let event = prop_oneof![
            3 => proptest::collection::vec(0..24u64, 0..12).prop_map(Event::Add),
            2 => (0..24u64, 0..5u64).prop_map(|(ino, size)| Event::Grow(ino, size)),
            2 => proptest::collection::vec(0..24u64, 0..12).prop_map(Event::Remove),
            1 => (0..24u64, 0..24u64).prop_map(|(old, new)| Event::Rename(old, new)),
            1 => Just(Event::Complete),
            3 => any::<bool>().prop_map(Event::Sort),
        ];
        proptest!(|(events in proptest::collection::vec(event, 1..60))| {
            let mut listing = Listing::default();
            for event in events {
                match event {
                    Event::Add(inos) => {
                        listing.apply(ListingEvent::Added(
                            inos.into_iter().map(|ino| entry(ino, 1)).collect(),
                        ));
                    }
                    Event::Grow(ino, size) => {
                        listing.apply(ListingEvent::Updated(vec![entry(ino, size)]));
                    }
                    Event::Remove(inos) => {
                        listing.apply(ListingEvent::Removed(inos.into_iter().map(id).collect()));
                    }
                    Event::Rename(old, new) => {
                        listing.apply(ListingEvent::Renamed(vec![(id(old), entry(new, 1))]));
                    }
                    Event::Complete => {
                        listing.apply(ListingEvent::Complete);
                    }
                    Event::Sort(by_size) => {
                        let spec = SortSpec {
                            key: if by_size { SortKey::Size } else { SortKey::Name },
                            ..SortSpec::default()
                        };
                        let shown = listing.sorted(spec);
                        if listing.complete {
                            prop_assert_eq!(&*shown, &*listing.compute_sorted(spec));
                        } else {
                            // While it streams, what arrived or moved is
                            // placed by quarters: the order lags, but holds
                            // only what is listed, once each.
                            let held: std::collections::HashSet<_> = shown.iter().collect();
                            prop_assert_eq!(held.len(), shown.len());
                            prop_assert!(shown.iter().all(|id| listing.entries.contains_key(id)));
                        }
                    }
                }
            }
        });
    }
}
