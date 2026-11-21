//! Settings: a TOML file, validated against the schema plugins register,
//! read through typed accessors. Plugins own the `plugin_id.*` namespace.
//!
//! The file is shared: another skwir, a hand edit or Home Manager may
//! change it while this one runs. A save writes the keys this process
//! changed over the file as it stands (`persist`), never its whole table,
//! and what changed in the file is taken in as it changes (`reload`,
//! watched by the app). What skwir keeps of the window as it was left
//! (`SettingSpec::state`) goes to a file of its own, in the state folder
//! (ADR-056): the settings file may then be read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};

use crate::command::Topic;
use crate::os_path::PathArg;
use crate::words::Words;
use serde::de::DeserializeOwned;
use std::io::Write;

pub const PRESERVE_ATTRIBUTES_ON_COPY: &str = "core.preserve_attributes_on_copy";
/// Whether files dropped without a key held ask what to do.
pub const DROP_ASKS: &str = "core.drop_asks";
/// Which pane closing the split takes away: `other`, `active` or `right`.
pub const CLOSE_SPLIT: &str = "core.close_split";
/// `last` or `folder`.
pub const STARTUP: &str = "core.startup";
pub const STARTUP_FOLDER: &str = "core.startup_folder";
/// Application data: where the window was when it closed.
pub const LAST_LOCATION: &str = "core.last_location";

/// The most read of a file of settings (one of skwir's, or one to check):
/// a larger one is not one.
const FILE_LIMIT: u64 = 16 << 20;

/// The Settings dialog's pages, by their subject (ADR-072): a setting
/// names the one it is shown on (`SettingSpec::page`), whoever owns its
/// key. The dialog draws each, and orders its sections.
pub mod page {
    /// What skwir does at startup, with tabs, the split, the address, and
    /// what is dragged and dropped.
    pub const GENERAL: &str = "general";
    pub const APPEARANCE: &str = "appearance";
    /// How folders show their files: the Narrow by bar, the grid, the
    /// list's columns, thumbnails.
    pub const VIEWS: &str = "views";
    /// Tags, then where notes are kept.
    pub const NOTES: &str = "notes";
    /// How documents, books, text and code read, and their engines.
    pub const READING: &str = "reading";
    /// Pictures, audio and video, 3D models and maps.
    pub const MEDIA: &str = "media";
    /// The terminal and the program commands run in.
    pub const TERMINAL: &str = "terminal";
    /// The servers of the sidebar's Network section, where those added by
    /// their address are kept, and the tools that open an address.
    pub const NETWORK: &str = "network";
    pub const PRIVACY: &str = "privacy";
    /// The folders skwir treats apart, and what each is kept from:
    /// searches, agents (ADR-125).
    pub const EXCLUSIONS: &str = "exclusions";
    pub const KEYBOARD: &str = "keyboard";
}

/// A group heading within a page ([`SettingSpec::section`]): told by its
/// id, which the settings of several owners meet by and a page orders its
/// sections by, and said by its words (ADR-139, 8). By subject, as pages
/// are, so they are core's ([`section`]); a section made for what a plugin
/// registers is named where it is made (`SettingSpec::section_of`).
#[derive(Clone, Copy, Debug)]
pub struct Section {
    pub id: &'static str,
    /// Its heading's words, made where they are said.
    pub title: fn() -> Words,
}

impl Section {
    pub const fn new(id: &'static str, title: fn() -> Words) -> Self {
        Section { id, title }
    }
}

/// Sections meet by their id (ADR-139, 8).
impl PartialEq for Section {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Section {}

/// The dialog's sections, by page.
pub mod section {
    use super::Section;

    // General.
    pub const LANGUAGE: Section = Section::new("language", || {
        crate::words!(
            settings_section_language = "Language",
            note = "A heading of a section of the settings."
        )
    });
    pub const STARTUP: Section = Section::new("startup", || {
        crate::words!(
            settings_section_startup = "Startup",
            note = "A heading of a section of the settings."
        )
    });
    pub const TABS: Section = Section::new("tabs", || {
        crate::words!(
            settings_section_tabs = "Tabs",
            note = "A heading of a section of the settings."
        )
    });
    pub const SPLIT: Section = Section::new("split", || {
        crate::words!(
            settings_section_split = "Split",
            note = "A heading of a section of the settings."
        )
    });
    pub const ADDRESS: Section = Section::new("address", || {
        crate::words!(
            settings_section_address = "Address",
            note = "A heading of a section of the settings."
        )
    });
    pub const DRAG_AND_DROP: Section = Section::new("drag_and_drop", || {
        crate::words!(
            settings_section_drag_and_drop = "Drag and drop",
            note = "A heading of a section of the settings."
        )
    });
    pub const ACTIONS: Section = Section::new("actions", || {
        crate::words!(
            settings_section_actions = "Actions",
            note = "A heading of a section of the settings."
        )
    });
    // Folders and views.
    pub const SORTING: Section = Section::new("sorting", || {
        crate::words!(
            settings_section_sorting = "Sorting",
            note = "A heading of a section of the settings."
        )
    });
    pub const SIZES: Section = Section::new("sizes", || {
        crate::words!(
            settings_section_sizes = "Sizes",
            note = "A heading of a section of the settings."
        )
    });
    pub const GRID: Section = Section::new("grid", || {
        crate::words!(
            settings_section_grid = "Grid",
            note = "A heading of a section of the settings."
        )
    });
    pub const LIST_COLUMNS: Section = Section::new("list_columns", || {
        crate::words!(
            settings_section_list_columns = "List columns",
            note = "A heading of a section of the settings."
        )
    });
    pub const THUMBNAILS: Section = Section::new("thumbnails", || {
        crate::words!(
            settings_section_thumbnails = "Thumbnails",
            note = "A heading of a section of the settings."
        )
    });
    pub const PREVIEWS: Section = Section::new("previews", || {
        crate::words!(
            settings_section_previews = "Previews",
            note = "A heading of a section of the settings."
        )
    });
    pub const GIT: Section = Section::new("git", || {
        crate::words!(
            settings_section_git = "Git",
            note = "A heading of a section of the settings."
        )
    });
    // Tags and notes.
    pub const TAGS: Section = Section::new("tags", || {
        crate::words!(
            settings_section_tags = "Tags",
            note = "A heading of a section of the settings."
        )
    });
    // Reading.
    pub const PAGES: Section = Section::new("pages", || {
        crate::words!(
            settings_section_pages = "Pages",
            note = "A heading of a section of the settings."
        )
    });
    pub const DOCUMENTS: Section = Section::new("documents", || {
        crate::words!(
            settings_section_documents = "Documents",
            note = "A heading of a section of the settings."
        )
    });
    pub const BOOKS: Section = Section::new("books", || {
        crate::words!(
            settings_section_books = "Books",
            note = "A heading of a section of the settings."
        )
    });
    pub const TEXT: Section = Section::new("text", || {
        crate::words!(
            settings_section_text = "Text",
            note = "A heading of a section of the settings."
        )
    });
    pub const CODE: Section = Section::new("code", || {
        crate::words!(
            settings_section_code = "Code",
            note = "A heading of a section of the settings."
        )
    });
    pub const GRAMMARS: Section = Section::new("grammars", || {
        crate::words!(
            settings_section_grammars = "Grammars",
            note = "A heading of a section of the settings."
        )
    });
    pub const SPREADSHEETS: Section = Section::new("spreadsheets", || {
        crate::words!(
            settings_section_spreadsheets = "Spreadsheets",
            note = "A heading of a section of the settings."
        )
    });
    // Pictures and media.
    pub const PICTURES: Section = Section::new("pictures", || {
        crate::words!(
            settings_section_pictures = "Pictures",
            note = "A heading of a section of the settings."
        )
    });
    pub const PLAYBACK: Section = Section::new("playback", || {
        crate::words!(
            settings_section_playback = "Playback",
            note = "A heading of a section of the settings."
        )
    });
    pub const AUDIO: Section = Section::new("audio", || {
        crate::words!(
            settings_section_audio = "Audio",
            note = "A heading of a section of the settings."
        )
    });
    pub const VIDEO: Section = Section::new("video", || {
        crate::words!(
            settings_section_video = "Video",
            note = "A heading of a section of the settings."
        )
    });
    pub const MODELS: Section = Section::new("models_3d", || {
        crate::words!(
            settings_section_models_3d = "3D models",
            note = "A heading of a section of the settings."
        )
    });
    pub const MAPS: Section = Section::new("maps", || {
        crate::words!(
            settings_section_maps = "Maps",
            note = "A heading of a section of the settings."
        )
    });
    // Terminal.
    pub const TERMINAL: Section = Section::new("terminal", || {
        crate::words!(
            settings_section_terminal = "Terminal",
            note = "A heading of a section of the settings."
        )
    });
    pub const COMMANDS: Section = Section::new("commands", || {
        crate::words!(
            settings_section_commands = "Commands",
            note = "A heading of a section of the settings."
        )
    });
    // Network.
    pub const SERVERS: Section = Section::new("servers", || {
        crate::words!(
            settings_section_servers = "Servers",
            note = "A heading of a section of the settings."
        )
    });
    pub const OPENING_ADDRESSES: Section = Section::new("opening_addresses", || {
        crate::words!(
            settings_section_opening_addresses = "Opening addresses",
            note = "A heading of a section of the settings."
        )
    });
}

/// The namespace of a dotted setting key: the owning plugin's id (or
/// `core`/`ui`). The one place that says which keys a plugin owns.
pub fn owner_of(key: &str) -> &str {
    key.split_once('.').map_or(key, |(owner, _)| owner)
}

/// How an editor should present a setting. The value's type is still the
/// type of `default`; the kind only picks the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    Toggle,
    /// One of `constraints.choices`.
    Choice,
    Integer,
    Number,
    Text,
    /// A filesystem path; editors offer completion or a picker.
    Path,
    /// A program to run: its name, looked for on the `PATH`, or its path.
    /// Run alone, never as a command line: `bash -l` is no program.
    Command,
    /// Application data managed by its owning commands, not by editors.
    Data,
    /// A list of records the user adds, edits and removes (tile servers):
    /// each a table of `constraints.fields` plus an `id`, edited as JSON.
    /// Empty by default, it is data the user built; one Skwir starts
    /// filled (`SettingSpec::starting_with`) is a preference, which Revert
    /// and Reset all give back.
    Records,
    /// A choice its owner keeps elsewhere (the desktop's default file
    /// manager, ADR-140): what it holds and offers is the owner's answer,
    /// and choosing runs the owner's command (see [`ChoiceSource`]). Never
    /// saved, never reverted: it is no preference of Skwir's.
    KeptChoice,
}

/// One field of a record in a [`SettingKind::Records`] setting.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordField {
    pub key: String,
    /// Its label and what it is, said where they are drawn; in English to
    /// agents and scripts (ADR-139, 7).
    pub title: Words,
    pub description: Words,
    pub kind: FieldKind,
    /// A record without it is refused.
    pub required: bool,
}

/// What a record's field holds, which picks its control.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    Text {
        /// What it shows while empty (an example), said where drawn.
        placeholder: Words,
    },
    Integer {
        min: i64,
        max: i64,
    },
    /// Name and value pairs (HTTP headers), stored as `[[name, value], …]`.
    Pairs,
    /// A secret kept in the desktop's keyring, never in the settings: the
    /// field holds only [`SECRET_STORED`] or nothing, and the keyring item
    /// is found by the setting's key, the record's id and the field's key
    /// (`skwir_platform::secrets`).
    Secret,
}

/// What a [`FieldKind::Secret`] field holds when the keyring has its value.
pub const SECRET_STORED: &str = "keyring";

impl RecordField {
    fn new(key: &str, title: Words, description: Words, kind: FieldKind) -> Self {
        RecordField {
            key: key.into(),
            title,
            description,
            kind,
            required: false,
        }
    }

    pub fn text(
        key: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        placeholder: impl Into<Words>,
    ) -> Self {
        Self::new(
            key,
            title.into(),
            description.into(),
            FieldKind::Text {
                placeholder: placeholder.into(),
            },
        )
    }

    pub fn integer(
        key: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        (min, max): (i64, i64),
    ) -> Self {
        Self::new(
            key,
            title.into(),
            description.into(),
            FieldKind::Integer { min, max },
        )
    }

    pub fn pairs(key: &str, title: impl Into<Words>, description: impl Into<Words>) -> Self {
        Self::new(key, title.into(), description.into(), FieldKind::Pairs)
    }

    pub fn secret(key: &str, title: impl Into<Words>, description: impl Into<Words>) -> Self {
        Self::new(key, title.into(), description.into(), FieldKind::Secret)
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// A button on each record that runs `command` with `{ "id": <record id> }`
/// (a tile server's Test).
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordAction {
    /// Its button's label, said where it is drawn.
    pub label: Words,
    pub command: String,
}

/// Records their owner keeps elsewhere (rclone's remotes, in rclone's own
/// configuration, ADR-086): the dialog lists what `list` answers
/// (`{ "records": [{ "id": …, <field>: … }] }`, dispatched on the window's
/// thread, so answered from the owner's memory; its words as data,
/// `Words::to_data`, which the dialog says and an agent reads in English,
/// what is no one's to translate as text) and changes them only
/// through the owner's commands, each run with `{ "id": <record id> }`
/// (`add` with nothing): never edited in place, never moved. The setting's
/// own value is never read nor saved. The dialog lists them again when
/// `revision` moves ([`RecordSource::changed`]).
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordSource {
    pub list: String,
    pub add: String,
    pub edit: String,
    pub remove: String,
    #[serde(skip)]
    pub revision: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl RecordSource {
    pub fn new(list: &str, add: &str, edit: &str, remove: &str) -> Self {
        RecordSource {
            list: list.into(),
            add: add.into(),
            edit: edit.into(),
            remove: remove.into(),
            revision: std::sync::Arc::default(),
        }
    }

    /// Say that what `list` answers changed: the dialog asks again.
    pub fn changed(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// What `changed` has moved it to.
    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// A choice its owner keeps elsewhere ([`SettingKind::KeptChoice`]): the
/// dialog draws what `list` answers, dispatched on the window's thread
/// and so answered from the owner's memory, `{ "value", "label",
/// "choices": [{ "value", "label", "detail"?, "disabled"? }], "command",
/// "args"?, "field", "detail"? }` (a record's `choice`, with what the
/// value means for the user in `detail`, said under the description; its
/// words as data, as a record's are), and
/// choosing runs `command` with `args` and the value chosen in `field`.
/// The setting's own value is never read nor saved: `settings.apply`
/// refuses it, as the owner's command changes it. The dialog lists it
/// again when `revision` moves ([`ChoiceSource::changed`]), which the
/// command choosing moves once it is answered, chosen, as it was or
/// refused: the dialog asks nothing more of the choice until then (a key
/// held down would ask again for what the answer holds already).
#[derive(Clone, Debug, serde::Serialize)]
pub struct ChoiceSource {
    pub list: String,
    #[serde(skip)]
    pub revision: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ChoiceSource {
    pub fn new(list: &str) -> Self {
        ChoiceSource {
            list: list.into(),
            revision: std::sync::Arc::default(),
        }
    }

    /// Say that what `list` answers changed: the dialog asks again.
    pub fn changed(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// What `changed` has moved it to.
    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The owner's own check of a value, after the kind's: what only it knows
/// (a tile address needs `{z}`, `{x}` and `{y}`). Runs wherever a value is
/// validated, in the dialog and in `settings.apply`, given the value
/// stored then, and in `settings.check`, given a file's value as it
/// stands ([`Beside`]): a check may spare what was already kept (a
/// shortcut for a plugin turned off, ADR-024).
#[derive(Clone)]
pub struct Validator(pub std::sync::Arc<ValidatorFn>);

/// What a [`Validator`] runs: the value, and what it is checked beside.
pub type ValidatorFn = dyn Fn(&toml::Value, Beside<'_>) -> anyhow::Result<()> + Send + Sync;

/// What a value is checked beside (see [`Validator`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Beside<'a> {
    /// A value to save over the one stored then (`None`: nothing is): the
    /// dialog's, `settings.apply`'s.
    Stored(Option<&'a toml::Value>),
    /// A file's value as it stands, nothing kept from before
    /// (`settings.check`): what may wait for a plugin that is off waits,
    /// what nothing could ever take is wrong.
    File,
}

impl std::fmt::Debug for Validator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Validator")
    }
}

/// A setting's description as its owner gives it (ADR-072, ADR-139): one
/// sentence, its row's, or that sentence and what follows it, two
/// messages, the first what the row shows, the second what its tooltip
/// adds: `(words!(…_description = "First."), words!(…_description_more =
/// "Then the rest."))`. Never one message cut where a sentence ends.
pub trait Description {
    /// Its first sentence, and what follows (none where it is empty).
    fn parts(self) -> (Words, Option<Words>);
}

impl Description for Words {
    fn parts(self) -> (Words, Option<Words>) {
        (self, None)
    }
}

impl Description for (Words, Words) {
    fn parts(self) -> (Words, Option<Words>) {
        let (first, more) = self;
        (first, Some(more).filter(|more| !more.is_empty()))
    }
}

impl Description for (Words, Option<Words>) {
    /// A first sentence and what may follow it, as an owner whose
    /// settings are alike gives them (a view's columns).
    fn parts(self) -> (Words, Option<Words>) {
        let (first, more) = self;
        (first, more.filter(|more| !more.is_empty()))
    }
}

impl Description for &str {
    /// Text as written (a test's).
    fn parts(self) -> (Words, Option<Words>) {
        (Words::as_written(self.to_owned()), None)
    }
}

impl Description for String {
    /// Text as written (a test's).
    fn parts(self) -> (Words, Option<Words>) {
        (Words::as_written(self), None)
    }
}

/// One setting's declaration. Build it with the constructors below: the
/// owning plugin states the title, unit, choices and limits, so no editor
/// needs key-specific knowledge.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SettingSpec {
    /// Dotted key, e.g. `view.list.row_height` or `plugin_id.option`.
    pub key: String,
    /// Short human label ("Memory budget"), said where it is drawn; in
    /// English to agents and scripts (ADR-139, 7).
    pub title: Words,
    /// Its description's first sentence, which its row shows (ADR-072).
    pub description: Words,
    /// What its description says after it, each a message of its own
    /// (`Description`): shown under it in the row's tooltip, and heard
    /// with it by a screen reader; none where one sentence says it all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<Words>,
    /// The dialog's page it is shown on (`page::*`), by its subject,
    /// whoever owns the key (ADR-072); empty for its owner's own page.
    pub page: String,
    /// Optional group heading within its page, by its id ([`Section`]):
    /// the settings of several owners meet by it. Sections stand in the
    /// order the page lists them, then in the order their first setting
    /// was declared, after what has none.
    pub section: String,
    /// The words its section's heading says.
    pub section_title: Words,
    /// Shown while its page is folded: among the few settings most people
    /// come to the page for (ADR-072). The others wait under the page's
    /// More settings.
    pub essential: bool,
    /// The setting this one is shown under (its key), which it applies
    /// only with: a switch while it is on, a choice while it has one of
    /// `under_values`; empty for none. The dialog draws it indented
    /// beneath, unavailable otherwise ([`SettingSpec::applies_with`]).
    pub under: String,
    /// The values of the choice `under` names that it applies with
    /// ([`SettingSpec::under_choice`]); none under a switch, which it
    /// applies with on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub under_values: Vec<String>,
    /// The unit a number is written in ("64 MB", "1 line"): its owner's
    /// words placing the number (`SettingSpec::unit`).
    pub unit: Option<Unit>,
    pub default: toml::Value,
    pub kind: SettingKind,
    pub constraints: SettingConstraints,
    /// The change takes effect at the next start.
    pub restart_required: bool,
    /// The file it is kept in: the settings, or apart from them
    /// (`SettingSpec::state`, `SettingSpec::data`).
    pub store: Store,
    /// Drawn by its page's own editor (`drawn_by_page`): never a row, yet
    /// taken by `settings.apply` (its value as JSON) and checked in a file.
    #[serde(default)]
    pub page_drawn: bool,
    #[serde(skip)]
    pub validator: Option<Validator>,
    /// When its key was first declared in this process, which
    /// `Settings::declare` sets: a page shows its settings in that order,
    /// so the owner orders them by declaring them in it.
    #[serde(skip)]
    pub order: u64,
}

/// The unit a setting's number is written in: its owner's words placing
/// the number `n`, which agree with it where a language's words do (a
/// selector on `n`: one line, 5 lines). A whole number
/// arrives as a count; one written with a part (`unit_of`), as text.
#[derive(Clone, Copy)]
pub struct Unit(pub fn(crate::words::Arg) -> Words);

impl Unit {
    /// The number `n` written in it.
    pub fn of(self, n: impl Into<crate::words::Arg>) -> Words {
        (self.0)(n.into())
    }
}

impl std::fmt::Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unit({:?})", self.of("n").english())
    }
}

impl serde::Serialize for Unit {
    /// In English, with `n` where the number goes ("n MB"): what agents
    /// and scripts read (ADR-139, 7).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.of("n").english())
    }
}

/// The units several owners' settings are written in, core's so that they
/// are one message (`SettingSpec::unit`).
pub mod unit {
    use crate::words::{Arg, Words};

    /// Megabytes: "64 MB".
    pub fn megabytes(n: Arg) -> Words {
        crate::words!(
            settings_unit_megabytes = "{ $n }\u{a0}MB",
            n = n,
            note = "A setting's number in megabytes, after its field or slider (64 MB)."
        )
    }

    /// A percentage: "100 %".
    pub fn percent(n: Arg) -> Words {
        crate::words!(
            settings_unit_percent = "{ $n }%",
            n = n,
            note = "A setting's number in percent, after its slider (100%)."
        )
    }

    /// Seconds: "1.5 s".
    pub fn seconds(n: Arg) -> Words {
        crate::words!(
            settings_unit_seconds = "{ $n }\u{a0}s",
            n = n,
            note = "A setting's number in seconds, after its slider (1.5 s): n may be a decimal."
        )
    }
}

/// The file a setting is kept in (ADR-056, ADR-057).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    /// The settings file: preferences, which a configuration may declare,
    /// and link read-only.
    #[default]
    Settings,
    /// The state file (`folders::state`): the window as it was left,
    /// skwir's own.
    State,
    /// The data file (`folders::data`): what the user made in skwir, as
    /// saved searches, pinned tags and tags' colours.
    Data,
}

impl Store {
    /// Its file, for a message's selector (`{ $file -> [state] … }`).
    fn file_key(self) -> &'static str {
        match self {
            Store::Settings => "settings",
            Store::State => "state",
            Store::Data => "data",
        }
    }
}

/// Optional restrictions shared by editors and command validation.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SettingConstraints {
    pub choices: Vec<String>,
    /// Human labels parallel to `choices` (empty: show the values), said
    /// where they are drawn; in English to agents and scripts (ADR-139,
    /// 7).
    pub choice_labels: Vec<Words>,
    pub integer_range: Option<(i64, i64)>,
    /// Increment of a numeric stepper, and the stops of a slider.
    pub step: i64,
    /// A whole number shown as a slider over its range, in its steps,
    /// rather than typed (`SettingSpec::slider`): a value seen as it
    /// changes. Its value stays a number for agents and scripts.
    pub slider: bool,
    /// How many of the value make one `unit` where it is written
    /// (`SettingSpec::unit_of`): 1000 for milliseconds written in seconds.
    /// 0 or 1: the value is counted in its unit.
    pub per_unit: i64,
    /// The fields of a [`SettingKind::Records`] setting's records.
    pub fields: Vec<RecordField>,
    /// The words of the button adding a record ("Add tile server"), its
    /// owner's whole.
    pub record_add: Words,
    pub record_action: Option<RecordAction>,
    /// Records their owner keeps elsewhere, changed by its commands (see
    /// [`RecordSource`]).
    pub record_source: Option<RecordSource>,
    /// The choice of a [`SettingKind::KeptChoice`] setting, kept by its
    /// owner (see [`ChoiceSource`]).
    pub choice_source: Option<ChoiceSource>,
    /// The choices are what is known here and now: others may be found
    /// where skwir runs (the themes a user added to bat). A file's value
    /// outside them is noted by `Settings::check`, not refused. No words:
    /// none join them.
    pub more_choices: Words,
    /// What the default choice, Automatic, comes to here and now: one of
    /// `choices` (the theme's `dark`) or a name of its own (a font's
    /// family); empty when its owner does not say. The dialog shows it
    /// while the default is chosen (`SettingSpec::choice_shown`).
    pub now: String,
    /// What Automatic tries, in its order, each choice with the requirement
    /// it needs (`Plugin::requirements`, by name): it gives the first whose
    /// requirement is found, which the dialog knows once it probed them
    /// (`SettingSpec::with_found`). Empty for a choice that says `now`
    /// itself.
    pub tries: Vec<Tried>,
    /// The requirement a choice needs where Automatic does not try it
    /// (`SettingSpec::needs`: an engine used only when chosen), so that a
    /// missing one says the choice does nothing, as for those it tries.
    pub needs: Vec<Tried>,
    /// The choices that do nothing here and now, each with why
    /// (`SettingSpec::unavailable`): their owner says so, or the dialog
    /// found missing the requirement one tries (`SettingSpec::with_missing`).
    /// Offered dimmed with why, never chosen from the dialog; a value
    /// among them set otherwise (the file, an agent, a tool gone since) is
    /// kept, not refused: it may serve elsewhere, the file another
    /// machine's too, and its row says it does nothing here.
    pub unavailable: Vec<Unavailable>,
    /// What each choice looks like, parallel to `choices` (empty: none
    /// does): the dialog draws it in miniature before the value's name, in
    /// the list and on the control (`ui.colors_from`'s palettes). The
    /// value stays a plain choice for agents and scripts.
    pub swatches: Vec<Option<Swatch>>,
}

/// The colors a choice's miniature is drawn in, beside its name in the
/// dialog ([`SettingConstraints::swatches`]): a window's, as sRGB
/// `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Swatch {
    /// What is read is on (a list of files), and the chrome around it.
    pub ground: u32,
    pub chrome: u32,
    pub text: u32,
    pub muted: u32,
    pub line: u32,
    pub accent: u32,
    /// What is chosen, and its text.
    pub selection: u32,
    pub selection_text: u32,
    /// Outcomes: done, a warning, a failure.
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    /// Whether it is a dark look.
    pub dark: bool,
}

/// A choice that does nothing here and now, and why
/// ([`SettingConstraints::unavailable`]).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Unavailable {
    pub choice: String,
    /// Said where it is drawn; in English to agents and scripts.
    pub why: Words,
}

/// A choice Automatic tries ([`SettingConstraints::tries`]) and the
/// requirement of its owner it needs.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Tried {
    pub choice: String,
    pub requires: String,
}

impl SettingSpec {
    fn new(key: &str, title: Words, description: impl Description, default: toml::Value) -> Self {
        let (description, more) = description.parts();
        let kind = match default {
            toml::Value::Boolean(_) => SettingKind::Toggle,
            toml::Value::Integer(_) => SettingKind::Integer,
            toml::Value::Float(_) => SettingKind::Number,
            toml::Value::String(_) => SettingKind::Text,
            _ => SettingKind::Data,
        };
        SettingSpec {
            key: key.into(),
            title,
            description,
            more,
            page: String::new(),
            section: String::new(),
            section_title: Words::as_written(""),
            essential: false,
            under: String::new(),
            under_values: Vec::new(),
            unit: None,
            default,
            kind,
            constraints: SettingConstraints {
                step: 1,
                ..SettingConstraints::default()
            },
            restart_required: false,
            store: Store::Settings,
            page_drawn: false,
            validator: None,
            order: 0,
        }
    }

    pub fn toggle(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: bool,
    ) -> Self {
        Self::new(
            key,
            title.into(),
            description,
            toml::Value::Boolean(default),
        )
    }

    /// `choices` are `(stored value, label)` pairs, each label as written
    /// (a name the system or the user gave, or words not yet a message).
    pub fn choice(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: &str,
        choices: &[(&str, &str)],
    ) -> Self {
        Self::choice_words(
            key,
            title,
            description,
            default,
            choices
                .iter()
                .map(|(value, label)| (*value, Words::as_written((*label).to_owned())))
                .collect(),
        )
    }

    /// `choices` are `(stored value, label)` pairs, each label words said
    /// where the choice is drawn (`words!`).
    pub fn choice_words(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: &str,
        choices: Vec<(&str, Words)>,
    ) -> Self {
        let mut spec = Self::new(
            key,
            title.into(),
            description,
            toml::Value::String(default.into()),
        );
        spec.kind = SettingKind::Choice;
        let (values, labels): (Vec<String>, Vec<Words>) = choices
            .into_iter()
            .map(|(value, label)| (value.to_owned(), label))
            .unzip();
        spec.constraints.choices = values;
        spec.constraints.choice_labels = labels;
        spec
    }

    pub fn integer(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: i64,
        range: (i64, i64),
    ) -> Self {
        let mut spec = Self::new(
            key,
            title.into(),
            description,
            toml::Value::Integer(default),
        );
        spec.constraints.integer_range = Some(range);
        spec
    }

    pub fn number(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: f64,
    ) -> Self {
        Self::new(key, title.into(), description, toml::Value::Float(default))
    }

    pub fn text(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: &str,
    ) -> Self {
        Self::new(
            key,
            title.into(),
            description,
            toml::Value::String(default.into()),
        )
    }

    pub fn path(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: &str,
    ) -> Self {
        let mut spec = Self::text(key, title, description, default);
        spec.kind = SettingKind::Path;
        spec
    }

    /// A program to run (`SettingKind::Command`): a value with words after
    /// a name is refused, which would not start (`bash -l`), saying why. A
    /// path may hold spaces.
    pub fn command(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: &str,
    ) -> Self {
        let mut spec = Self::text(key, title, description, default).validate_with(|value, _| {
            let program = value.as_str().unwrap_or_default().trim();
            anyhow::ensure!(
                !program.contains(char::is_whitespace)
                    || std::path::Path::new(program).is_absolute(),
                crate::words!(
                    settings_program_alone = "Give the program alone, without arguments: “bash”, not “bash -l”",
                    note = "Refused where a setting names a program to run: the examples are commands, as typed."
                )
            );
            Ok(())
        });
        spec.kind = SettingKind::Command;
        spec
    }

    /// A preference whose page draws it with an editor of its own (the
    /// exclusions' tables, ADR-125): kept in the settings file, as any
    /// preference, checked by its owner (`validate_with`), never drawn as
    /// a row.
    pub fn drawn_by_page(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        default: toml::Value,
    ) -> Self {
        let mut spec = Self::new(key, title.into(), description, default);
        spec.kind = SettingKind::Data;
        spec.page_drawn = true;
        spec
    }

    /// What the user makes in skwir rather than chooses in its Settings
    /// (saved searches, pinned tags, tags' colours): kept in the data file,
    /// apart from the settings (ADR-057), so it is kept when the settings
    /// are read-only. Not shown in the dialog.
    pub fn data(key: &str, description: impl Into<Words>, default: toml::Value) -> Self {
        // Never a row: no title (ADR-139, 8), its description for whoever
        // reads the file.
        let mut spec = Self::new(
            key,
            Words::default(),
            Into::<Words>::into(description),
            default,
        );
        spec.kind = SettingKind::Data;
        spec.store = Store::Data;
        spec
    }

    /// What skwir keeps of the window as it was left (its layout, its tabs,
    /// the hints it showed): data, in the state file rather than the
    /// settings (ADR-056), so the settings may be read-only.
    pub fn state(key: &str, description: impl Into<Words>, default: toml::Value) -> Self {
        let mut spec = Self::data(key, description, default);
        spec.store = Store::State;
        spec
    }

    /// A list of records, each made of `fields` (see [`SettingKind::Records`]);
    /// `add`, the words of the button adding one ("Add tile server").
    pub fn records(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        add: impl Into<Words>,
        fields: Vec<RecordField>,
    ) -> Self {
        let mut spec = Self::new(
            key,
            title.into(),
            description,
            toml::Value::Array(Vec::new()),
        );
        spec.kind = SettingKind::Records;
        spec.constraints.fields = fields;
        spec.constraints.record_add = add.into();
        spec
    }

    /// A choice its owner keeps elsewhere, as `source` lists it (see
    /// [`ChoiceSource`]): drawn as a choice's row, chosen through the
    /// owner's command, never saved here.
    pub fn choice_kept_by(
        key: &str,
        title: impl Into<Words>,
        description: impl Description,
        source: ChoiceSource,
    ) -> Self {
        let mut spec = Self::new(
            key,
            title.into(),
            description,
            toml::Value::String(String::new()),
        );
        spec.kind = SettingKind::KeptChoice;
        spec.constraints.choice_source = Some(source);
        spec
    }

    /// What lists a setting its owner keeps elsewhere, a list of records
    /// or a choice, and where it stands: the command the dialog asks
    /// (from the owner's memory) and the revision it asks again at.
    pub fn kept_source(&self) -> Option<(&str, u64)> {
        if let Some(source) = &self.constraints.record_source {
            return Some((source.list.as_str(), source.revision()));
        }
        self.constraints
            .choice_source
            .as_ref()
            .map(|source| (source.list.as_str(), source.revision()))
    }

    /// The records the list starts with, Skwir's, in their order: each
    /// given an id of its own, its place in the list (`1`, `2`…), which no
    /// record added since takes (`settings_records::new_id`). A list
    /// changed from them is a preference changed: Revert and Reset all
    /// give them back.
    #[must_use]
    pub fn starting_with(mut self, records: impl IntoIterator<Item = toml::Table>) -> Self {
        self.default = toml::Value::Array(
            records
                .into_iter()
                .enumerate()
                .map(|(at, mut record)| {
                    record.insert("id".into(), toml::Value::String((at + 1).to_string()));
                    toml::Value::Table(record)
                })
                .collect(),
        );
        self
    }

    /// Whether `value` is other than the records the list starts with,
    /// their ids aside: a list built from nothing never is (it is data),
    /// and one whose records were removed and typed again as they were
    /// is not.
    pub fn records_changed(&self, value: &toml::Value) -> bool {
        fn without_ids(value: &toml::Value) -> Vec<toml::Table> {
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_table)
                .map(|record| {
                    let mut record = record.clone();
                    record.remove("id");
                    record
                })
                .collect()
        }
        let start = without_ids(&self.default);
        !start.is_empty() && without_ids(value) != start
    }

    /// A button on each record, running `command` with the record's id.
    #[must_use]
    pub fn record_action(mut self, label: impl Into<Words>, command: &str) -> Self {
        self.constraints.record_action = Some(RecordAction {
            label: label.into(),
            command: command.into(),
        });
        self
    }

    /// The records are kept by their owner elsewhere, listed and changed
    /// through its commands (see [`RecordSource`]): the list is never the
    /// settings'.
    #[must_use]
    pub fn records_kept_by(mut self, source: RecordSource) -> Self {
        self.constraints.record_source = Some(source);
        self
    }

    /// The owner's own check, after the kind's (see [`Validator`]): of
    /// the value, beside the one stored or as a file holds it.
    #[must_use]
    pub fn validate_with<F>(mut self, check: F) -> Self
    where
        F: Fn(&toml::Value, Beside<'_>) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        self.validator = Some(Validator(std::sync::Arc::new(check)));
        self
    }

    /// What the default choice, Automatic, comes to now: a value of its
    /// choices or a name (`SettingConstraints::now`). The owner declares
    /// the setting again when it changes.
    #[must_use]
    pub fn now(mut self, now: &str) -> Self {
        now.clone_into(&mut self.constraints.now);
        self
    }

    /// What Automatic tries, in its order: each choice and the name of the
    /// requirement it needs, one of its owner's (`SettingConstraints::tries`).
    #[must_use]
    pub fn tries(mut self, tries: &[(&str, &str)]) -> Self {
        self.constraints.tries = tries
            .iter()
            .map(|(choice, requires)| Tried {
                choice: (*choice).to_owned(),
                requires: (*requires).to_owned(),
            })
            .collect();
        self
    }

    /// Say that `choices` do nothing here and now, each with why, as its
    /// owner knows (`SettingConstraints::unavailable`).
    #[must_use]
    pub fn unavailable<'a>(mut self, choices: impl IntoIterator<Item = (&'a str, Words)>) -> Self {
        for (choice, why) in choices {
            self.mark_unavailable(choice, why);
        }
        self
    }

    /// The requirement each of `choices` needs, where Automatic does not
    /// try it (`SettingConstraints::needs`).
    #[must_use]
    pub fn needs(mut self, choices: &[(&str, &str)]) -> Self {
        self.constraints
            .needs
            .extend(choices.iter().map(|(choice, requires)| Tried {
                choice: (*choice).to_owned(),
                requires: (*requires).to_owned(),
            }));
        self
    }

    /// The choices whose requirement is missing, as the dialog probed them
    /// (`missing`: why, by the requirement's name), those Automatic tries
    /// and those that need one (`needs`), said unavailable.
    #[must_use]
    pub fn with_missing(mut self, missing: impl Fn(&str) -> Option<Words>) -> Self {
        let tried = self.constraints.tries.iter().chain(&self.constraints.needs);
        for tried in tried.cloned().collect::<Vec<_>>() {
            if let Some(why) = missing(&tried.requires) {
                self.mark_unavailable(&tried.choice, why);
            }
        }
        self
    }

    fn mark_unavailable(&mut self, choice: &str, why: Words) {
        let unavailable = &mut self.constraints.unavailable;
        if !unavailable.iter().any(|u| u.choice == choice) {
            unavailable.push(Unavailable {
                choice: choice.to_owned(),
                why,
            });
        }
    }

    /// Why `choice` does nothing here and now, where it does nothing.
    pub fn unavailable_why(&self, choice: &str) -> Option<&Words> {
        self.constraints
            .unavailable
            .iter()
            .find(|u| u.choice == choice)
            .map(|u| &u.why)
    }

    /// The choice `delta` steps from `from` among those that do something
    /// here, the ends kept: `from` itself where none does that way.
    pub fn step_choice(&self, from: &str, delta: i64) -> String {
        let choices = &self.constraints.choices;
        let Some(start) = choices.iter().position(|c| c == from) else {
            return choices
                .iter()
                .find(|c| self.unavailable_why(c).is_none())
                .cloned()
                .unwrap_or_else(|| from.to_owned());
        };
        let (mut at, mut left, mut last) = (start, delta.unsigned_abs(), start);
        while left > 0 {
            let next = if delta > 0 {
                at.checked_add(1)
            } else {
                at.checked_sub(1)
            };
            let Some(next) = next.filter(|n| *n < choices.len()) else {
                break;
            };
            at = next;
            if self.unavailable_why(&choices[at]).is_none() {
                last = at;
                left -= 1;
            }
        }
        choices[last].clone()
    }

    /// The setting saying what Automatic gives as `found` answers for each
    /// of its owner's requirements it tries: the first found. Unchanged
    /// when its owner says it (`now`) or none is found.
    #[must_use]
    pub fn with_found(mut self, found: impl Fn(&str) -> bool) -> Self {
        if self.constraints.now.is_empty()
            && let Some(tried) = self.constraints.tries.iter().find(|t| found(&t.requires))
        {
            self.constraints.now.clone_from(&tried.choice);
        }
        self
    }

    /// What each of its choices looks like (`SettingConstraints::swatches`),
    /// parallel to its choices.
    #[must_use]
    pub fn swatches(mut self, swatches: Vec<Option<Swatch>>) -> Self {
        self.constraints.swatches = swatches;
        self
    }

    /// What `value` looks like, when its owner says.
    pub fn swatch_of(&self, value: &str) -> Option<Swatch> {
        let index = self.constraints.choices.iter().position(|c| c == value)?;
        self.constraints.swatches.get(index).copied().flatten()
    }

    /// Choices others may join where skwir runs, said in a few words
    /// ("themes you added to bat"), placed in a sentence: see
    /// `SettingConstraints::more_choices`.
    #[must_use]
    pub fn more_choices(mut self, which: impl Into<Words>) -> Self {
        self.constraints.more_choices = which.into();
        self
    }

    /// Under `section`'s heading on its page.
    #[must_use]
    pub fn section(self, section: Section) -> Self {
        self.section_of(section.id, (section.title)())
    }

    /// Under the heading of a section made for what a plugin registers
    /// (a notes store's): its id, which no other owner's settings share,
    /// and its words.
    #[must_use]
    pub fn section_of(mut self, id: impl Into<String>, title: impl Into<Words>) -> Self {
        self.section = id.into();
        self.section_title = title.into();
        self
    }

    /// Shown on `page` (one of [`page`]), by its subject, rather than on
    /// its owner's own page.
    #[must_use]
    pub fn page(mut self, page: &str) -> Self {
        page.clone_into(&mut self.page);
        self
    }

    /// Among the few settings shown while its page is folded (ADR-072):
    /// people are divided on its value, it decides what other programs or
    /// machines see, or it acts on files without asking. A budget keeps
    /// them few (`crates/skwir/tests/settings.rs`).
    #[must_use]
    pub fn essential(mut self) -> Self {
        self.essential = true;
        self
    }

    /// Shown under the switch `parent`, applying only while it is on (the
    /// desktop's font under the desktop's colors). Declare it right after
    /// the switch, in the same section.
    #[must_use]
    pub fn under(mut self, parent: &str) -> Self {
        parent.clone_into(&mut self.under);
        self.under_values.clear();
        self
    }

    /// Shown under the choice `parent`, applying only while it has one of
    /// `values` (the startup folder under Open at startup's A chosen
    /// folder). Declare it right after the choice, in the same section.
    #[must_use]
    pub fn under_choice(mut self, parent: &str, values: &[&str]) -> Self {
        parent.clone_into(&mut self.under);
        self.under_values = values.iter().map(|&value| value.to_owned()).collect();
        self
    }

    /// Whether it applies while the setting it is under has `value`, as
    /// the dialog writes values (`true` for a switch on, a choice's
    /// value): a switch's on, else one of its choice's values.
    pub fn applies_with(&self, value: &str) -> bool {
        if self.under_values.is_empty() {
            value == "true"
        } else {
            self.under_values.iter().any(|v| v == value)
        }
    }

    /// Written in `unit`, words placing the number `n`, a selector on it
    /// where they agree with it: `|n| words!(terminal_lines = "{ $n ->
    /// [one] … *[other] … }", n = n)`.
    #[must_use]
    pub fn unit(mut self, unit: fn(crate::words::Arg) -> Words) -> Self {
        self.unit = Some(Unit(unit));
        self
    }

    /// Written in `unit`, `per` of the value making one, a power of ten: a
    /// slider of milliseconds that says "1.5 s" (`unit_of(seconds,
    /// 1000)`), the number placed as text. The value stays the whole
    /// number kept, for agents and scripts; the key says what it counts
    /// (`_ms`).
    #[must_use]
    pub fn unit_of(mut self, unit: fn(crate::words::Arg) -> Words, per: i64) -> Self {
        debug_assert!(
            per >= 1 && 10_i64.pow(per.ilog10()) == per,
            "{}: a unit is a power of ten of the value",
            self.key
        );
        self.unit = Some(Unit(unit));
        self.constraints.per_unit = per.max(1);
        self
    }

    /// `value` as it is written, in its unit (`unit_of`): "64 MB", "1.5 s".
    #[must_use]
    pub fn written(&self, value: i64) -> Words {
        let number = self.figures(value);
        match self.unit {
            Some(unit) => unit.of(number),
            None => Words::as_written(match number {
                crate::words::Arg::Count(whole) => whole.to_string(),
                crate::words::Arg::Text(text) => text.into_owned(),
                _ => value.to_string(),
            }),
        }
    }

    /// `value` in its unit, without it: a count where it is a whole one,
    /// "1.5" as text for 1500 milliseconds written in seconds. A decimal
    /// is written as English writes it until numbers arrive with each
    /// language (PLAN.md, step F).
    fn figures(&self, value: i64) -> crate::words::Arg {
        let per = self.constraints.per_unit.max(1);
        let (whole, part) = (value / per, value.abs() % per);
        if part == 0 {
            return whole.into();
        }
        let sign = if value < 0 { "-" } else { "" };
        // The part's digits, as many as `per` has places, without the
        // zeros ending them.
        let places = per.ilog10() as usize;
        let part = format!("{part:0places$}");
        format!("{sign}{}.{}", whole.abs(), part.trim_end_matches('0')).into()
    }

    #[must_use]
    pub fn step(mut self, step: i64) -> Self {
        self.constraints.step = step.max(1);
        self
    }

    /// A whole number in a short range, shown in the dialog as a slider
    /// over it, its knob stopping at each `step` (an opacity in fives):
    /// for a value judged as it changes rather than one known by its
    /// number (a memory budget). Declared with `integer`.
    #[must_use]
    pub fn slider(mut self) -> Self {
        debug_assert!(
            self.constraints.integer_range.is_some(),
            "{}: a slider needs a range",
            self.key
        );
        self.constraints.slider = true;
        self
    }

    #[must_use]
    pub fn restart_required(mut self) -> Self {
        self.restart_required = true;
        self
    }

    /// The settings namespace, which is the owning plugin's id (or `core`/`ui`).
    pub fn owner(&self) -> &str {
        owner_of(&self.key)
    }

    /// The label of a choice value, falling back to the value itself (as
    /// written).
    pub fn choice_label(&self, value: &str) -> Words {
        self.constraints
            .choices
            .iter()
            .position(|choice| choice == value)
            .and_then(|index| self.constraints.choice_labels.get(index))
            .map_or_else(|| Words::as_written(value.to_owned()), Clone::clone)
    }

    /// The choice `value` as the window shows it: its label, and for the
    /// default, while its owner says what it comes to (`now`), that too:
    /// "Automatic (Dark)", "Automatic (Noto Sans)".
    pub fn choice_shown(&self, value: &str) -> Words {
        let label = self.choice_label(value);
        match self.now_of(value) {
            Some(now) => crate::words!(
                settings_choice_now = "{ $choice } ({ $now })",
                choice = label,
                now = now,
                note = "A setting's default choice and what it comes to here and now: Automatic (Dark)."
            ),
            None => label,
        }
    }

    /// What the choice `value` comes to now, as shown: the label of the
    /// choice it is, or its own name; `None` unless `value` is the default
    /// and its owner says, or it comes to itself.
    pub fn now_of(&self, value: &str) -> Option<Words> {
        let now = self.constraints.now.as_str();
        let default = self.default.as_str()?;
        (value == default && !now.is_empty() && now != default).then(|| self.choice_label(now))
    }

    /// Collections are application data (e.g. saved queries), managed by their
    /// owning commands rather than the preferences editor.
    pub fn editable(&self) -> bool {
        self.kind != SettingKind::Data
    }

    /// The text an editor shows and sends back: a string as itself, a list
    /// or a table (records) as JSON, anything else as TOML writes it.
    pub fn format_value(value: &toml::Value) -> String {
        match value {
            toml::Value::String(text) => text.clone(),
            toml::Value::Array(_) | toml::Value::Table(_) => {
                serde_json::to_string(value).unwrap_or_default()
            }
            _ => value.to_string(),
        }
    }

    /// Records from their JSON text, each checked against the fields. The
    /// dialog edits a record by its id, which each needs there; a file's
    /// (`Beside::File`, Home Manager's) may leave them out, as what reads
    /// the records does (shortcuts, private folders).
    fn parse_records(&self, text: &str, beside: Beside<'_>) -> anyhow::Result<toml::Value> {
        let records: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(if text.trim().is_empty() { "[]" } else { text })
                .map_err(|error| {
                    anyhow::Error::new(crate::words!(
                        settings_expected_records = "expected a list of records: { $error }",
                        error = error.to_string(),
                        note = "A list of records given as JSON could not be read; the reader's own words follow."
                    ))
                })?;
        let mut ids = std::collections::BTreeSet::new();
        let mut out = Vec::with_capacity(records.len());
        for record in records {
            let id = record
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty());
            anyhow::ensure!(
                id.is_some() || matches!(beside, Beside::File),
                crate::words!(settings_record_needs_id = "every record needs an id")
            );
            let mut table = toml::Table::new();
            if let Some(id) = id {
                anyhow::ensure!(
                    ids.insert(id.to_owned()),
                    crate::words!(
                        settings_records_share_id = "two records share the id { $id }",
                        id = id,
                        note = "The id is an identifier, as written."
                    )
                );
                table.insert("id".into(), toml::Value::String(id.into()));
            }
            for field in &self.constraints.fields {
                // The field's words, said where the error is.
                let title = || field.title.clone();
                let value = record.get(&field.key).filter(|v| !v.is_null());
                let empty = match value {
                    None => true,
                    Some(serde_json::Value::String(s)) => s.trim().is_empty(),
                    Some(serde_json::Value::Array(a)) => a.is_empty(),
                    Some(_) => false,
                };
                anyhow::ensure!(
                    !(field.required && empty),
                    crate::words!(
                        settings_field_needed = "{ $title } is needed",
                        title = title(),
                        note = "A record's field left empty: its title."
                    )
                );
                let Some(value) = value.filter(|_| !empty) else {
                    continue;
                };
                let converted = match &field.kind {
                    FieldKind::Text { .. } => toml::Value::String(
                        value
                            .as_str()
                            .ok_or_else(|| {
                                anyhow::Error::new(crate::words!(
                                    settings_field_text = "{ $title } is text",
                                    title = title()
                                ))
                            })?
                            .trim()
                            .to_owned(),
                    ),
                    FieldKind::Integer { min, max } => {
                        let n = value
                            .as_i64()
                            .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
                            .ok_or_else(|| {
                                anyhow::Error::new(crate::words!(
                                    settings_field_whole_number = "{ $title } is a whole number",
                                    title = title()
                                ))
                            })?;
                        anyhow::ensure!(
                            (*min..=*max).contains(&n),
                            crate::words!(
                                settings_field_range =
                                    "{ $title }: enter a whole number from { $min } to { $max }",
                                title = title(),
                                min = min.to_string(),
                                max = max.to_string(),
                                note = "A record's field that takes a whole number: its title, the least and the most it takes."
                            )
                        );
                        toml::Value::Integer(n)
                    }
                    FieldKind::Pairs => {
                        let pairs = value.as_array().ok_or_else(|| {
                            anyhow::Error::new(crate::words!(
                                settings_field_pairs = "{ $title } is a list of pairs",
                                title = title()
                            ))
                        })?;
                        let mut list = Vec::with_capacity(pairs.len());
                        for pair in pairs {
                            let [name, value] =
                                pair.as_array().map(Vec::as_slice).unwrap_or_default()
                            else {
                                anyhow::bail!(crate::words!(
                                    settings_field_name_value_pairs =
                                        "{ $title } is a list of name and value pairs",
                                    title = title()
                                ));
                            };
                            let (Some(name), Some(value)) = (name.as_str(), value.as_str()) else {
                                anyhow::bail!(crate::words!(
                                    settings_field_pairs_text =
                                        "{ $title }: names and values are text",
                                    title = title()
                                ));
                            };
                            if name.trim().is_empty() && value.trim().is_empty() {
                                continue;
                            }
                            anyhow::ensure!(
                                !name.trim().is_empty(),
                                crate::words!(
                                    settings_field_value_needs_name =
                                        "{ $title }: a value needs a name",
                                    title = title()
                                )
                            );
                            list.push(toml::Value::Array(vec![
                                toml::Value::String(name.trim().into()),
                                toml::Value::String(value.trim().into()),
                            ]));
                        }
                        if list.is_empty() {
                            continue;
                        }
                        toml::Value::Array(list)
                    }
                    FieldKind::Secret => {
                        // The secret itself goes to the keyring; the settings
                        // file only says that it is there.
                        anyhow::ensure!(
                            value.as_str() == Some(SECRET_STORED),
                            crate::words!(
                                settings_field_secret =
                                    "{ $title } is kept in the keyring, not in the settings",
                                title = title()
                            )
                        );
                        toml::Value::String(SECRET_STORED.into())
                    }
                };
                table.insert(field.key.clone(), converted);
            }
            out.push(toml::Value::Table(table));
        }
        Ok(toml::Value::Array(out))
    }

    /// Whether `value`, read from a file rather than typed, is one this
    /// setting takes: its kind, then whatever the dialog checks of what is
    /// typed (choices, ranges, the owner's own check). Data takes any
    /// value of its default's kind; its owner reads it.
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn check(&self, value: &toml::Value) -> anyhow::Result<()> {
        let kind = |value: &toml::Value| match value {
            toml::Value::String(_) => "string",
            toml::Value::Integer(_) => "integer",
            toml::Value::Float(_) => "float",
            toml::Value::Boolean(_) => "boolean",
            toml::Value::Datetime(_) => "datetime",
            toml::Value::Array(_) => "array",
            toml::Value::Table(_) => "table",
        };
        let fits = std::mem::discriminant(value) == std::mem::discriminant(&self.default)
            || matches!(
                (&self.default, value),
                (toml::Value::Float(_), toml::Value::Integer(_))
            );
        anyhow::ensure!(
            fits,
            crate::words!(
                settings_expected_kind = "expected { $want ->
                    [integer] a whole number
                    [float] a number
                    [boolean] true or false
                    [datetime] a date
                    [array] a list
                    [table] a table
                   *[string] text
                }, not { $got ->
                    [integer] a whole number
                    [float] a number
                    [boolean] true or false
                    [datetime] a date
                    [array] a list
                    [table] a table
                   *[string] text
                }",
                want = kind(&self.default),
                got = kind(value),
                note = "A settings file's value of the wrong kind: what the setting takes, then what the file holds."
            )
        );
        if !self.editable() && !self.page_drawn {
            return Ok(());
        }
        self.parse_beside(&Self::format_value(value), Beside::File)
            .map(|_| ())
    }

    /// The value `text` says, checked as if nothing were stored.
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn parse(&self, text: &str) -> anyhow::Result<toml::Value> {
        self.parse_over(text, None)
    }

    /// The value `text` says, to replace `stored`: the owner's check sees
    /// both (see [`Validator`]).
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn parse_over(
        &self,
        text: &str,
        stored: Option<&toml::Value>,
    ) -> anyhow::Result<toml::Value> {
        self.parse_beside(text, Beside::Stored(stored))
    }

    /// Why `key` is not changed as a preference.
    fn managed(key: &str) -> Words {
        crate::words!(
            settings_managed = "{ $key } is managed by its owning commands",
            key = key,
            note = "The key is a setting's, as a file writes it."
        )
    }

    /// What a switch's value is to be.
    fn expected_boolean() -> Words {
        crate::words!(
            settings_expected_boolean = "expected true or false",
            note = "As a settings file writes a switch's value: true and false are its words."
        )
    }

    fn parse_beside(&self, text: &str, beside: Beside<'_>) -> anyhow::Result<toml::Value> {
        anyhow::ensure!(self.editable() || self.page_drawn, Self::managed(&self.key));
        // Kept by its owner: chosen through its command, never saved here.
        anyhow::ensure!(
            self.kind != SettingKind::KeptChoice,
            Self::managed(&self.key)
        );
        let value = match self.default {
            _ if self.kind == SettingKind::Records => self.parse_records(text, beside)?,
            // A table or a list its page draws, as JSON (`format_value`).
            toml::Value::Table(_) | toml::Value::Array(_) if self.page_drawn => {
                let json: serde_json::Value =
                    serde_json::from_str(if text.trim().is_empty() { "null" } else { text })
                        .map_err(|error| {
                            anyhow::Error::new(crate::words!(
                                settings_expected_json = "expected JSON: { $error }",
                                error = error.to_string(),
                                note = "A value given as JSON could not be read; the reader's own words follow."
                            ))
                        })?;
                match json {
                    serde_json::Value::Null => self.default.clone(),
                    json => toml::Value::try_from(json).map_err(|error| {
                        anyhow::Error::new(crate::words!(
                            settings_not_file_value = "not a value a file holds: { $error }",
                            error = error.to_string(),
                            note = "A JSON value a settings file cannot hold; the writer's own words follow."
                        ))
                    })?,
                }
            }
            toml::Value::String(_) => toml::Value::String(text.into()),
            toml::Value::Boolean(_) => toml::Value::Boolean(
                text.trim()
                    .parse()
                    .map_err(|_| anyhow::Error::new(Self::expected_boolean()))?,
            ),
            toml::Value::Integer(_) => toml::Value::Integer(text.trim().parse().map_err(|_| {
                anyhow::Error::new(crate::words!(
                    settings_expected_whole_number = "expected a whole number"
                ))
            })?),
            toml::Value::Float(_) => {
                let number: f64 = text.trim().parse().map_err(|_| {
                    anyhow::Error::new(crate::words!(
                        settings_expected_number = "expected a number"
                    ))
                })?;
                anyhow::ensure!(
                    number.is_finite(),
                    crate::words!(settings_expected_finite = "expected a finite number")
                );
                toml::Value::Float(number)
            }
            _ => anyhow::bail!(Self::managed(&self.key)),
        };
        anyhow::ensure!(
            self.constraints.choices.is_empty()
                || self
                    .constraints
                    .choices
                    .iter()
                    .any(|s| Some(s.as_str()) == value.as_str()),
            crate::words!(
                settings_choose_one = "choose one of { $choices }",
                choices =
                    crate::words::List::or(self.constraints.choices.iter().map(String::as_str)),
                note = "The values a setting takes, as a file writes them, joined as one of them is chosen (light, dark or auto)."
            )
        );
        if let Some((min, max)) = self.constraints.integer_range {
            anyhow::ensure!(
                value.as_integer().is_some_and(|n| (min..=max).contains(&n)),
                crate::words!(
                    settings_range = "enter a whole number from { $min } to { $max }",
                    min = min.to_string(),
                    max = max.to_string(),
                    note = "A setting that takes a whole number: the least and the most it takes."
                )
            );
        }
        if let Some(Validator(check)) = &self.validator {
            check(&value, beside)?;
        }
        Ok(value)
    }
}

/// What `Settings::check` found: problems, by key, and notes on keys it
/// cannot judge, said where they are shown; in English to agents and
/// scripts (ADR-139, 7).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Checked {
    pub problems: Vec<(String, Words)>,
    pub notes: Vec<(String, Words)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SaveState {
    #[default]
    Idle,
    Saving,
    Saved,
    /// Why the save failed, said where it is shown.
    Failed(Words),
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CheckArgs {
    /// A settings file, by absolute path.
    #[serde(default)]
    file: Option<PathArg>,
    /// Settings as a table (JSON), in place of a file.
    #[serde(default)]
    settings: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ApplyArgs {
    /// Dotted setting keys and their editor text (strings need no TOML quotes).
    changes: BTreeMap<String, String>,
}

/// Register the nonblocking settings command. Validation is repeated on the
/// worker in case a plugin was disabled after the command was queued.
pub(crate) fn register(app: &crate::App) {
    app.settings.declare(
        SettingSpec::choice_words(
            STARTUP,
            crate::words!(settings_startup = "Open at startup"),
            crate::words!(
                settings_startup_description =
                    "What Skwir opens when it starts with no folder given on the command line."
            ),
            "folder",
            vec![
                (
                    "folder",
                    crate::words!(settings_startup_folder_choice = "A chosen folder"),
                ),
                (
                    "last",
                    crate::words!(settings_startup_last = "The windows and tabs left open"),
                ),
            ],
        )
        .section(section::STARTUP)
        .essential(),
    );
    app.settings.declare(
        SettingSpec::path(
            STARTUP_FOLDER,
            crate::words!(settings_startup_folder = "Startup folder"),
            (
                crate::words!(
                    settings_startup_folder_description = "Used when a chosen folder opens at startup."
                ),
                crate::words!(
                    settings_startup_folder_description_more = "Leave empty for your home folder.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            "",
        )
        // A folder chosen is read only when the startup opens one.
        .under_choice(STARTUP, &["folder"])
        .section(section::STARTUP)
        .essential(),
    );
    app.settings.declare(
        SettingSpec::toggle(
            PRESERVE_ATTRIBUTES_ON_COPY,
            keep_attributes_title(),
            (
                crate::words!(
                    settings_keep_attributes_description = "Copies carry their source's tags, rating, comment and where it was downloaded from."
                ),
                crate::words!(
                    settings_keep_attributes_description_more = "Where the destination keeps no attributes they are kept beside the copy, as a Mac does; the copy fails only when nothing can keep them. Turn this off to copy without them.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            true,
        )
        .page(page::NOTES)
        .section(section::TAGS),
    );
    app.settings.declare(
        SettingSpec::toggle(
            DROP_ASKS,
            crate::words!(settings_drop_asks = "Ask what dropped files do"),
            (
                crate::words!(
                    settings_drop_asks_description = "Files dropped in a folder with no key held open a menu: move, copy or link them there.",
                    note = "Shift, Ctrl and Ctrl+Shift are keys, as the keyboard names them."
                ),
                crate::words!(
                    settings_drop_asks_description_more = "Turn this off to move them within a drive and copy them to another. Shift moves, Ctrl copies and Ctrl+Shift links, either way.",
                    note = "Shift, Ctrl and Ctrl+Shift are keys, as the keyboard names them."
                ),
            ),
            true,
        )
        // Folded: General's fourth essential is the default file manager
        // (ADR-072, amended 2026-11-19).
        .section(section::DRAG_AND_DROP),
    );
    app.settings.declare(
        SettingSpec::toggle(
            crate::companions::SHOW,
            crate::words!(settings_show_companions = "Show companion files"),
            (
                crate::words!(
                    settings_show_companions_description = "With hidden files shown, also list the files that keep another file's notes beside it: a Mac's ._ files, XMP and TagSpaces sidecars.",
                    note = "._ is how a Mac's companion files begin their names; Properties is a dialog."
                ),
                crate::words!(
                    settings_show_companions_description_more = "Properties lists a file's companions either way.",
                    note = "._ is how a Mac's companion files begin their names; Properties is a dialog."
                ),
            ),
            false,
        )
        .page(page::NOTES)
        .section(section::TAGS),
    );
    app.companions
        .set_shown(app.settings.get(crate::companions::SHOW).unwrap_or(false));
    app.settings.declare(
        SettingSpec::toggle(
            crate::location::FOLDERS_FIRST,
            crate::words!(settings_folders_first = "Folders first"),
            (
                crate::words!(
                    settings_folders_first_description = "List folders before files, each sorted among themselves."
                ),
                crate::words!(
                    settings_folders_first_description_more = "Off sorts them together: by date modified, the newest comes first, folder or file. By size, folders, which are not weighed, sort as having none.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            true,
        )
        .page(page::VIEWS)
        .section(section::SORTING)
        .essential(),
    );
    apply_folders_first(app);
    app.settings.declare(
        SettingSpec::choice_words(
            crate::fs::SIZE_UNITS_KEY,
            crate::words!(settings_size_units = "Units", note = "How sizes are counted: the setting's title."),
            (
                crate::words!(
                    settings_size_units_description = "How sizes are counted.",
                    note = "kB, MB, KiB… are units' symbols; 10M, 10MB and 10MiB are as a search is typed, never translated."
                ),
                crate::words!(
                    settings_size_units_description_more = "Decimal counts a kilobyte as 1000 bytes (kB, MB, GB), as disks are sold and as macOS and GNOME show sizes; Binary counts 1024 (KiB, MiB, GiB), as KDE does. A search reads 10M as 1024-based either way, 10MB as 1000 and 10MiB as 1024.",
                    note = "kB, MB, KiB… are units' symbols; 10M, 10MB and 10MiB are as a search is typed, never translated."
                ),
            ),
            crate::fs::SizeUnits::Decimal.key(),
            vec![
                (
                    crate::fs::SizeUnits::Decimal.key(),
                    crate::words!(settings_size_units_decimal = "Decimal"),
                ),
                (
                    crate::fs::SizeUnits::Binary.key(),
                    crate::words!(settings_size_units_binary = "Binary"),
                ),
            ],
        )
        .page(page::VIEWS)
        .section(section::SIZES)
        .essential(),
    );
    apply_size_units(app);
    app.settings.declare(
        SettingSpec::choice_words(
            crate::locale::LANGUAGE_KEY,
            crate::words!(settings_language = "Language"),
            (
                crate::words!(
                    settings_language_description = "The language Skwir's window speaks.",
                    note = "Automatic is a choice of the setting (settings_language_automatic)."
                ),
                crate::words!(
                    settings_language_description_more = "Automatic speaks the first of your system's languages Skwir is translated into, else English; a translation still a draft says so. A language listed as incomplete is spoken only if you choose it, with what it lacks in English. Agents and scripts are answered in English whichever you choose.",
                    note = "Automatic is a choice of the setting (settings_language_automatic). A translation is a draft until a person who speaks the language has reviewed nearly all of it (settings_language_draft); an incomplete one, not yet translated, is listed with how much of it is translated and reviewed (settings_language_incomplete)."
                ),
            ),
            crate::locale::AUTOMATIC,
            language_choices(),
        )
        // What Automatic speaks here: the first system language offered.
        .now(&crate::locale::automatic())
        .page(page::GENERAL)
        .section(section::LANGUAGE)
        .essential(),
    );
    apply_language(app);
    app.settings.declare(
        SettingSpec::choice_words(
            CLOSE_SPLIT,
            crate::words!(settings_close_split = "Pane that closes"),
            (
                crate::words!(
                    settings_close_split_description = "Which pane goes when you close the split with Split or F3.",
                    note = "Split is a command's name; Other, Active and Right are the setting's choices."
                ),
                crate::words!(
                    settings_close_split_description_more = "Other keeps the pane you are in. Active closes it and keeps the other. Right always closes the right pane, or the lower one when the panes are stacked.",
                    note = "Split is a command's name; Other, Active and Right are the setting's choices."
                ),
            ),
            "other",
            vec![
                (
                    "other",
                    crate::words!(settings_close_split_other = "Other", note = "Which pane closes: the other one."),
                ),
                (
                    "active",
                    crate::words!(settings_close_split_active = "Active", note = "Which pane closes: the one you are in."),
                ),
                (
                    "right",
                    crate::words!(settings_close_split_right = "Right", note = "Which pane closes: the right one."),
                ),
            ],
        )
        .section(section::SPLIT),
    );
    app.settings.declare(SettingSpec::state(
        LAST_LOCATION,
        crate::words!(
            settings_last_location = "The folder shown when the window was last closed",
            note = "What a key of the state file holds, for whoever reads it."
        ),
        toml::Value::String(String::new()),
    ));
    app.commands
        .register::<CheckArgs, _>(
            "core", Topic::Skwir,
            "settings.check",
            crate::words!(settings_check = "Check settings"),
            crate::words!(
                settings_check_description = "Check a settings file, or settings given as a table, against the settings this build of Skwir declares: fails with what is wrong (a value a setting does not take, a key no setting has), and notes the keys of owners it does not know",
                note = "Read by agents and in F1's list of actions."
            ),
            |args, app| {
                let table: toml::Table = match (&args.file, &args.settings) {
                    (Some(file), None) => {
                        let path = crate::os_path::absolute(file)?;
                        // Whatever an agent names: a pipe or a device is no
                        // settings file, nor is one larger than any.
                        let text = crate::fs::file::read_regular_text(path, FILE_LIMIT).map_err(|error| {
                            anyhow::Error::new(crate::words!(
                                settings_check_unreadable = "{ $path } cannot be read: { $why }",
                                path = crate::words::Arg::Path(path.display().to_string()),
                                why = crate::op::io_words(&error),
                                note = "A settings file to check: its path, then why it cannot be read."
                            ))
                        })?;
                        text.parse()
                            .map_err(|error: toml::de::Error| {
                                anyhow::Error::new(crate::words!(
                                    settings_check_invalid = "{ $path } is not valid: { $problem }",
                                    path = crate::words::Arg::Path(path.display().to_string()),
                                    problem = toml_problem(&text, &error),
                                    note = "A settings file to check that is no TOML: its path, then what is wrong and where."
                                ))
                            })?
                    }
                    (None, Some(settings)) => toml::Table::try_from(settings).map_err(|error| {
                        anyhow::Error::new(crate::words!(
                            settings_check_not_settings = "not settings: { $error }",
                            error = error.to_string(),
                            note = "Settings given as a table that a settings file cannot hold; the writer's own words follow."
                        ))
                    })?,
                    _ => anyhow::bail!(crate::words!(
                        settings_check_one = "give a file or settings, one of them",
                        note = "Said to an agent that gave both a file and settings, or neither."
                    )),
                };
                let plugins: Vec<String> =
                    app.plugins.list().into_iter().map(|plugin| plugin.id.to_owned()).collect();
                let checked = app.settings.check(&table, &plugins);
                if !checked.problems.is_empty() {
                    let said: Vec<String> = checked
                        .problems
                        .iter()
                        .map(|(key, problem)| format!("{key}: {problem}"))
                        .collect();
                    anyhow::bail!("{}", said.join("; "));
                }
                Ok(crate::command::Outcome::Value(serde_json::to_value(&checked)?))
            },
        )
        .data();
    app.commands.register::<ApplyArgs, _>(
        "core", Topic::Skwir,
        "settings.apply",
        crate::words!(settings_apply = "Save settings"),
        crate::words!(
            settings_apply_description = "Change preferences, each key with the text its row in Settings takes; all of them are saved, or none",
            note = "Read by agents and in F1's list of actions."
        ),
        |args, app| {
            if !crate::scheduler::has_window()
                && let Some(reason) = &app.settings.saving_unavailable
            {
                // An interactive session can warn and keep temporary values.
                // A headless command cannot claim it saved those values.
                anyhow::bail!(reason.clone());
            }
            let read = app.settings.validate(&args.changes)?;
            // Only the user lowers privacy (ADR-125).
            crate::exclusions::refuse_exposure(&app.settings, &read)?;
            let mut status = app.settings.status.lock();
            anyhow::ensure!(
                status.1 != SaveState::Saving,
                crate::words!(settings_saving_already = "settings are already being saved")
            );
            status.0 += 1;
            status.1 = SaveState::Saving;
            let id = status.0;
            drop(status);
            // The guard owns the Saving state: whether the job runs, panics, or
            // is dropped unexecuted (cancelled lifetime), the state machine
            // always leaves Saving, so a later apply is never rejected forever.
            let mut guard = SaveGuard {
                weak: app.weak(),
                id,
                state: None,
            };
            let save = move || -> anyhow::Result<()> {
                let app = guard.weak.upgrade()
                    .ok_or_else(|| anyhow::Error::new(save_did_not_run()))?;
                let result = app.settings.apply(&args.changes).map(|keys| {
                    changed(&app, &keys);
                });
                // The error kept for both: the state says it, and a caller
                // without a window is answered with it.
                let result = result.map_err(std::sync::Arc::new);
                guard.state = Some(match &result {
                    Ok(()) => SaveState::Saved,
                    Err(error) => SaveState::Failed(Words::of_error_from(error, 0)),
                });
                drop(guard);
                result.map_err(|error| anyhow::Error::new(Words::of_error_from(&error, 0)))
            };
            if crate::scheduler::has_window() {
                // The window follows SaveState, including saves requested by
                // its control connection. Plugin callbacks can stop that
                // connection, so it must not wait for them on its own runtime.
                app.scheduler.spawn(
                    crate::scheduler::Priority::Normal,
                    "settings-save",
                    app.lifetime().child(),
                    move || { let _ = save(); },
                );
            } else {
                // A headless caller has no event loop to observe completion
                // and may exit as soon as dispatch returns. Save before
                // answering, and report persistence failures to that caller.
                save()?;
            }
            Ok(crate::command::Outcome::Value(serde_json::json!(id)))
        },
    )
    // The Settings window's save: no list of actions runs it.
    .asked_only();
}

/// The languages the setting offers: Automatic, English, each language the
/// program ships catalogs in by its own name, and, in a debug build, the
/// two pseudo-languages: the accented one that shows what is not yet a
/// message (`words::PSEUDO`), and the right-to-left one that mirrors the
/// window (`direction::PSEUDO_RTL`).
/// A language Skwir does not offer yet (ADR-139, 13) says so, with how
/// much of it there is: Automatic never picks it, and it is for whoever
/// chooses it. One Automatic speaks as a draft says it is one, with how
/// much of it a person reviewed.
fn language_choices() -> Vec<(&'static str, Words)> {
    let mut choices = vec![
        (
            crate::locale::AUTOMATIC,
            crate::words!(
                settings_language_automatic = "Automatic",
                note = "The language chosen by the system's languages."
            ),
        ),
        // A language by its own name, in every language.
        (crate::locale::ENGLISH, Words::as_written("English")),
    ];
    // Each language the program ships, named by its own catalog: the
    // message that says the language's name in itself.
    for language in crate::words::shipped_languages() {
        let own = language_name(language);
        let share = crate::words::share_of(language);
        let offer = share.map_or(crate::words::Offer::Incomplete, |share| share.offered);
        if offer == crate::words::Offer::Reviewed {
            choices.push((language, own));
            continue;
        }
        let (translated, reviewed) = share.map_or((0, 0), |share| {
            (share.translated_percent(), share.reviewed_percent())
        });
        if offer == crate::words::Offer::Draft {
            choices.push((
                language,
                crate::words!(
                    settings_language_draft = "{ $language } — draft, { $reviewed }% reviewed",
                    language = own,
                    reviewed = reviewed,
                    note = "A language of the Language setting that Automatic speaks, translated but not yet reviewed enough: its name in itself (Français), then how much of Skwir's words a person who speaks it reviewed, in percents of all of them."
                ),
            ));
            continue;
        }
        choices.push((
            language,
            crate::words!(
                settings_language_incomplete = "{ $language } — incomplete, { $translated }% translated, { $reviewed }% reviewed",
                language = own,
                translated = translated,
                reviewed = reviewed,
                note = "A language of the Language setting that Automatic does not speak yet: its name in itself (Français), then how much of Skwir's words it has, in percents of all of them. Reviewed words were accepted by a person who speaks the language; the others are drafts."
            ),
        ));
    }
    if cfg!(debug_assertions) {
        choices.push((
            crate::words::PSEUDO,
            crate::words!(
                settings_language_pseudo = "Pseudo (for testing)",
                note = "The pseudo-language, English accented and longer, offered in builds for testing."
            ),
        ));
        choices.push((
            crate::direction::PSEUDO_RTL,
            crate::words!(
                settings_language_pseudo_rtl = "Pseudo, right to left (for testing)",
                note = "The right-to-left pseudo-language, English drawn as a right-to-left language is, the window mirrored, offered in builds for testing."
            ),
        ));
    }
    choices
}

/// The name of the language a catalog is in (`fr`, `zh-Hans`), in itself
/// whatever the window speaks (Français, 简体中文): as the Language setting
/// and About name it. Counted with its share by `just words`, so that
/// listing the languages reads none of their catalogs; English, the code's
/// own, and a catalog shipped uncounted are said from their words.
pub fn language_name(language: &str) -> Words {
    if let Some(share) = crate::words::share_of(language) {
        return Words::as_written(share.name);
    }
    Words::as_written(
        crate::words!(
            settings_language_self = "English",
            note = "This language's name in itself, as the Language setting lists it in every language: Français in the French catalog, Deutsch in the German one."
        )
        .say_in(&[language.to_owned()]),
    )
}

/// A settings file's key no setting has.
fn no_such_setting() -> Words {
    crate::words!(settings_no_such = "no such setting")
}

/// Why a save of the settings says nothing of how it went.
fn save_did_not_run() -> Words {
    crate::words!(settings_save_did_not_run = "settings save did not run")
}

/// Speak the language the setting chooses (`locale`, ADR-139, 6).
fn apply_language(app: &crate::App) {
    let value = app
        .settings
        .get::<String>(crate::locale::LANGUAGE_KEY)
        .unwrap_or_default();
    crate::locale::set_chosen(crate::locale::chosen_of(&value));
}

/// Count sizes as the setting says (`fs::size_units`, ADR-139, 9).
fn apply_size_units(app: &crate::App) {
    let units = app
        .settings
        .get::<String>(crate::fs::SIZE_UNITS_KEY)
        .unwrap_or_default();
    crate::fs::set_size_units(crate::fs::SizeUnits::of_key(&units));
}

/// Give every pane, and every pane made from now, the user's choice of
/// folders before files (`location::FOLDERS_FIRST`).
fn apply_folders_first(app: &crate::App) {
    let first = app
        .settings
        .get(crate::location::FOLDERS_FIRST)
        .unwrap_or(true);
    app.workspace.write().set_folders_first(first);
}

/// What follows a change of `keys`, made in the dialog or found in the
/// file (`Settings::reload`): what the core keeps of them is set again, a
/// plugin turned on or off in the file is, and the plugins hear of theirs.
/// On a worker.
pub(crate) fn changed(app: &crate::App, keys: &[String]) {
    if keys.iter().any(|key| key == crate::tabs::SLEEP_KEY) {
        app.apply_tabs_sleep();
    }
    if keys
        .iter()
        .any(|key| owner_of(key) == crate::notes::SETTINGS)
    {
        app.apply_notes_settings();
    }
    if keys.iter().any(|key| key == crate::companions::SHOW) {
        app.companions
            .set_shown(app.settings.get(crate::companions::SHOW).unwrap_or(false));
        for pane in app.workspace.write().panes_mut() {
            pane.companions_changed();
        }
        app.wake();
    }
    if keys.iter().any(|key| key == crate::location::FOLDERS_FIRST) {
        apply_folders_first(app);
        app.wake();
    }
    // What reads the languages reads them anew (`locale::revision`).
    if keys.iter().any(|key| key == crate::locale::LANGUAGE_KEY) {
        apply_language(app);
        app.wake();
    }
    // Every size is said anew where it is drawn.
    if keys.iter().any(|key| key == crate::fs::SIZE_UNITS_KEY) {
        apply_size_units(app);
        app.wake();
    }
    if keys.iter().any(|key| {
        key == crate::orders::ORDERS
            || key == crate::orders::DIALOG_ORDERS
            || key == crate::orders::DIALOG_ORDER
    }) {
        crate::orders::apply(app);
        app.wake();
    }
    if keys
        .iter()
        .any(|key| key == crate::exclusions::SHELL_VARIABLES_KEY)
    {
        crate::environment::forget();
    }
    if keys.iter().any(|key| {
        key == crate::exclusions::ENTRIES_KEY || key == crate::exclusions::CATEGORIES_KEY
    }) {
        crate::exclusions::tell_problems(app);
    }
    for key in keys {
        if let Some(id) = key
            .strip_prefix("plugins.")
            .and_then(|rest| rest.strip_suffix(".enabled"))
            && let Some(enabled) = app.settings.get::<bool>(key)
            && app.plugins.is_enabled(id) != enabled
        {
            app.plugins.set_enabled(id, enabled, app);
        }
    }
    app.plugins.settings_changed(keys, app);
}

/// Take in what changed in the settings file and the data file, then
/// follow them: the folders holding them watched until the app ends. On a
/// worker: a folder is made if missing, and watching may wait for its
/// drive.
pub(crate) fn follow(app: &std::sync::Arc<crate::App>) {
    if app.settings.path().is_none() {
        return;
    }
    // The files followed, by folder: one watch where they share one.
    let mut folders: BTreeMap<PathBuf, Vec<std::ffi::OsString>> = BTreeMap::new();
    for path in [app.settings.path(), app.settings.data_path()]
        .into_iter()
        .flatten()
    {
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            folders
                .entry(dir.to_path_buf())
                .or_default()
                .push(name.to_os_string());
        }
    }
    for (dir, names) in folders {
        if let Err(error) = std::fs::create_dir_all(&dir) {
            tracing::warn!(%error, "a folder of the settings cannot be made: not followed");
            continue;
        }
        let (scheduler, weak, cancel) = (app.scheduler.clone(), app.weak(), app.lifetime().child());
        let reloading = cancel.clone();
        let watched = crate::fs::watch::watch(
            &dir,
            std::time::Duration::from_millis(200),
            &cancel,
            move |changes| {
                if !changes.rescan && !names.iter().any(|name| changes.names.contains(name)) {
                    return;
                }
                let weak = weak.clone();
                // Read on a worker, never on the watcher's thread.
                scheduler.spawn(
                    crate::scheduler::Priority::Background,
                    "settings-reload",
                    reloading.clone(),
                    move || {
                        let Some(app) = weak.upgrade() else { return };
                        if let Some(keys) = app.settings.reload()
                            && !keys.is_empty()
                        {
                            changed(&app, &keys);
                            app.wake();
                        }
                    },
                );
            },
        );
        match watched {
            Ok(handle) => app.settings.followed.lock().push(handle),
            Err(error) => tracing::warn!(%error, "a file of the settings cannot be followed"),
        }
    }
}

/// Publishes the outcome of one save, or a failure if the job never completed.
struct SaveGuard {
    weak: std::sync::Weak<crate::App>,
    id: u64,
    state: Option<SaveState>,
}

impl Drop for SaveGuard {
    fn drop(&mut self) {
        let Some(app) = self.weak.upgrade() else {
            return;
        };
        let state = self
            .state
            .take()
            .unwrap_or_else(|| SaveState::Failed(save_did_not_run()));
        let mut status = app.settings.status.lock();
        if status.0 == self.id {
            status.1 = state;
        }
        drop(status);
        app.wake();
    }
}

/// Where a window opens when the command line names no folder: the last
/// folder or the chosen one, per `core.startup`, falling back to `home`.
/// The chosen one is typed: `~` is `home`, as in the address field.
pub fn startup_folder(settings: &Settings, home: &Path) -> PathBuf {
    let stored = |key: &str| {
        settings
            .get::<PathArg>(key)
            .map(PathArg::into_inner)
            .filter(|path| !path.as_os_str().is_empty())
    };
    let typed = |path: PathBuf| match path.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => home.to_path_buf(),
        Ok(rest) => home.join(rest),
        Err(_) => path,
    };
    let choice = match settings.get::<String>(STARTUP).as_deref() {
        Some("last") => stored(LAST_LOCATION),
        _ => stored(STARTUP_FOLDER).map(typed),
    };
    choice.unwrap_or_else(|| home.to_path_buf())
}

/// Record `folder` as the one to reopen, when the startup preference
/// says to come back to it (`tabs::remember`, at exit). Whether it changed;
/// the caller saves.
pub fn remember_location(settings: &Settings, folder: &Path) -> bool {
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return false;
    }
    // A string when the path is UTF-8, its bytes otherwise (`os_path`): a
    // folder whose name is not UTF-8 reopens as itself.
    if settings.get::<PathArg>(LAST_LOCATION).as_deref() == Some(folder) {
        return false;
    }
    let Ok(value) = toml::Value::try_from(PathArg::from(folder)) else {
        return false;
    };
    settings.set(LAST_LOCATION, value);
    true
}

/// Loaded settings plus declared schema.
pub struct Settings {
    path: Option<PathBuf>,
    /// The files kept apart from the settings (`with_state`, `with_data`):
    /// the keys declared for one are kept there, or saved nowhere when it
    /// is `None` (a data file that could neither be read nor set aside). A
    /// store not here keeps its keys in the settings file, with the rest.
    apart: BTreeMap<Store, Option<PathBuf>>,
    /// What each file kept apart held when last read or written: a key
    /// declared kept apart takes its value from there whole (`declare`),
    /// over a copy the settings file may still hold (an earlier skwir's, a
    /// hand's), which is read only until skwir keeps its own.
    apart_read: Mutex<BTreeMap<Store, toml::Table>>,
    /// The keys ever declared kept apart in this process, and where: a
    /// plugin turned off keeps them where they were.
    kept: RwLock<BTreeMap<String, Store>>,
    values: RwLock<toml::Table>,
    schema: RwLock<BTreeMap<String, SettingSpec>>,
    /// Every key declared in this process, by when it first was
    /// (`SettingSpec::order`): declared again, or by a plugin turned on
    /// again, a setting keeps its place.
    declared: Mutex<BTreeMap<String, u64>>,
    /// Each key moved in this process (`rename`), old name to new: a file
    /// still saying the old one is told where it went (`check`).
    renamed: Mutex<BTreeMap<String, String>>,
    /// Serializes writers (readers never wait for file I/O), and holds the
    /// keys this process changed since its last save: what a save writes
    /// over the file as it stands.
    writer: Mutex<BTreeSet<String>>,
    /// The watches on the settings' and the data's folders (`follow`),
    /// while the app runs.
    followed: Mutex<Vec<crate::fs::watch::WatchHandle>>,
    status: Mutex<(u64, SaveState)>,
    /// Advances when the schema or a live value changes, so editors compare
    /// one atomic instead of re-reading the schema on every frame. What
    /// skwir keeps of the window (the state file, written as it changes,
    /// ADR-122) is no setting, and nothing is derived from it: its keys do
    /// not move this, or each folder changed would work out the theme, the
    /// keymap and the privacy rules again.
    revision: std::sync::atomic::AtomicU64,
    /// Why the file could not be read, until the app says it
    /// (`take_problem`).
    problem: Mutex<Option<Words>>,
    /// A file-backed instance fell back to memory after a load failure.
    /// Kept even after the startup warning is consumed; unlike an explicit
    /// in-memory instance, headless preference changes cannot claim a save.
    saving_unavailable: Option<Words>,
    /// Owners whose declarations are refused, the registries' (`gated`):
    /// a plugin turned off declares nothing, from a worker of it too.
    gate: std::sync::Arc<crate::registry::OwnerGate>,
}

/// A TOML error in a few words, with its line: "expected `=` on line 3".
fn toml_problem(text: &str, error: &toml::de::Error) -> Words {
    let message = error.message().trim_end_matches('.');
    match error.span() {
        Some(span) => {
            let line = text
                .get(..span.start)
                .map_or(1, |before| before.matches('\n').count() + 1);
            crate::words!(
                settings_toml_on_line = "{ $problem } on line { $line }",
                problem = message,
                line = line.to_string(),
                note = "What the TOML reader found wrong in a settings file, in its own words, and the line it is on."
            )
        }
        None => Words::as_written(message.to_owned()),
    }
}

/// Move an unreadable settings file out of the way, beside itself, under
/// a name no other one has: `settings.toml.unreadable-<seconds>`.
fn set_aside(path: &Path) -> std::io::Result<PathBuf> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".unreadable-{seconds}"));
    let aside = path.with_file_name(name);
    std::fs::rename(path, &aside)?;
    Ok(aside)
}

impl Settings {
    /// The system's settings file of skwir's
    /// (`$XDG_CONFIG_HOME/skwir/settings.toml` on Linux): in
    /// `crate::folders::config`.
    pub fn default_path() -> PathBuf {
        crate::folders::config()
            .unwrap_or_else(|| std::env::temp_dir().join("skwir"))
            .join("settings.toml")
    }

    /// The system's file of skwir's state (`$XDG_STATE_HOME/skwir/state.toml`
    /// on Linux): in `crate::folders::state`.
    pub fn default_state_path() -> Option<PathBuf> {
        crate::folders::state().map(|dir| dir.join("state.toml"))
    }

    /// In-memory settings with no file.
    pub fn in_memory() -> Self {
        Settings {
            path: None,
            apart: BTreeMap::new(),
            apart_read: Mutex::new(BTreeMap::new()),
            kept: RwLock::new(BTreeMap::new()),
            values: RwLock::new(toml::Table::new()),
            schema: RwLock::new(BTreeMap::new()),
            declared: Mutex::new(BTreeMap::new()),
            renamed: Mutex::new(BTreeMap::new()),
            writer: Mutex::new(BTreeSet::new()),
            followed: Mutex::new(Vec::new()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
            problem: Mutex::new(None),
            saving_unavailable: None,
            gate: std::sync::Arc::default(),
        }
    }

    /// Load from `path`; a missing file yields empty settings. skwir always
    /// starts: a file that is no TOML (edited by hand, say) is set aside
    /// under a name of its own, never written over, and the defaults are
    /// used; one that cannot be read or set aside leaves the settings in
    /// memory, saved nowhere. `take_problem` says which.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (values, problem, saves) = Self::read_or_set_aside(&path, Store::Settings);
        Settings {
            path: saves.then_some(path),
            apart: BTreeMap::new(),
            apart_read: Mutex::new(BTreeMap::new()),
            kept: RwLock::new(BTreeMap::new()),
            values: RwLock::new(values),
            schema: RwLock::new(BTreeMap::new()),
            declared: Mutex::new(BTreeMap::new()),
            renamed: Mutex::new(BTreeMap::new()),
            writer: Mutex::new(BTreeSet::new()),
            followed: Mutex::new(Vec::new()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
            saving_unavailable: (!saves).then(|| problem.clone()).flatten(),
            problem: Mutex::new(problem),
            gate: std::sync::Arc::default(),
        }
    }

    /// Declarations refused as `gate` refuses contributions: the app's, the
    /// registries' gate (`Registry::gate`), which the plugin host closes
    /// before it withdraws a plugin turned off.
    #[must_use]
    pub fn gated(mut self, gate: std::sync::Arc<crate::registry::OwnerGate>) -> Self {
        self.gate = gate;
        self
    }

    /// The table in the file at `path` (empty when there is none), and
    /// what to tell the user when it could not be read: one that is no TOML
    /// set aside beside itself, never written over. False when nothing may
    /// be saved there: it could neither be read nor set aside. `whose`
    /// file it is words the problem: "Your settings could not be read (…),
    /// so skwir started with its defaults."
    fn read_or_set_aside(path: &Path, whose: Store) -> (toml::Table, Option<Words>, bool) {
        let whose = whose.file_key();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (toml::Table::new(), None, true);
            }
            Err(error) => {
                // Words, said once the settings chose the language.
                let problem = crate::words!(
                    settings_unreadable_unsaved = "{ $file ->
                        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them, and saves nothing there this time.
                       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults, and saves nothing there this time.
                    }",
                    file = whose,
                    why = crate::op::io_words(&error),
                    note = "Told at startup: a file of Skwir's could not be read; why, in a few words."
                );
                return (toml::Table::new(), Some(problem), false);
            }
        };
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|_| {
                crate::words!(
                    settings_not_text = "it is not text",
                    note = "Why a settings file could not be read: its bytes are no text."
                )
            })
            .and_then(|text| {
                text.parse::<toml::Table>()
                    .map_err(|e| toml_problem(text, &e))
            });
        let why = match parsed {
            Ok(values) => return (values, None, true),
            Err(why) => why,
        };
        tracing::warn!(path = %path.display(), "unreadable: {why}");
        match set_aside(path) {
            Ok(aside) => {
                let problem = crate::words!(
                    settings_unreadable_set_aside = "{ $file ->
                        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them. The file was set aside as { $aside }.
                       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults. The file was set aside as { $aside }.
                    }",
                    file = whose,
                    why = why,
                    aside = crate::words::Name::of(&aside),
                    note = "Told at startup: a file of Skwir's could not be read (why, in a few words) and was renamed (its new name)."
                );
                (toml::Table::new(), Some(problem), true)
            }
            Err(error) => {
                let problem = crate::words!(
                    settings_unreadable_kept = "{ $file ->
                        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them, and saves nothing there until { $path } is fixed or removed ({ $error }).
                       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults, and saves nothing there until { $path } is fixed or removed ({ $error }).
                    }",
                    file = whose,
                    why = why,
                    path = crate::words::Arg::Path(path.display().to_string()),
                    error = crate::op::io_words(&error),
                    note = "Told at startup: a file of Skwir's could not be read (why) nor renamed (its path, and why not)."
                );
                (toml::Table::new(), Some(problem), false)
            }
        }
    }

    /// The same settings, their state kept in the file at `path` (made at
    /// the first save): a key it holds is read from it, whole, over a copy
    /// the settings file may still have (`declare`). One that cannot be
    /// read is started over: it is skwir's own, and holds nothing the user
    /// chose.
    #[must_use]
    pub fn with_state(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match Self::on_disk(&path) {
            Ok(state) => {
                self.apart_read
                    .get_mut()
                    .insert(Store::State, state.clone());
                Self::merge_into(&mut self.values.write(), state);
            }
            Err(error) => tracing::warn!("the window's state is started over: {error:#}"),
        }
        self.apart.insert(Store::State, Some(path));
        self
    }

    /// The same settings, what the user made in skwir (`SettingSpec::data`:
    /// saved searches, pinned tags, tags' colours) kept in the file at
    /// `path` (ADR-057), made at the first save: a key it holds is read
    /// from it, whole, over a copy the settings file may still have
    /// (`declare`). It is the user's: one that is no TOML is set aside, as
    /// the settings file is, and said; one that can be neither read nor set
    /// aside is never written, what changes kept in memory until skwir
    /// starts again.
    #[must_use]
    pub fn with_data(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (data, problem, saves) = Self::read_or_set_aside(&path, Store::Data);
        self.apart_read.get_mut().insert(Store::Data, data.clone());
        Self::merge_into(&mut self.values.write(), data);
        if let Some(problem) = problem {
            let mut said = self.problem.lock();
            *said = Some(match said.take() {
                Some(before) => crate::words!(
                    settings_problems = "{ $first } { $then }",
                    first = before,
                    then = problem,
                    note = "Two problems told at startup, each a sentence or more: the settings', then the saved searches'."
                ),
                None => problem,
            });
        }
        self.apart.insert(Store::Data, saves.then_some(path));
        self
    }

    /// `other`'s values set in `base`, tables merged.
    fn merge_into(base: &mut toml::Table, other: toml::Table) {
        for (key, value) in other {
            match (base.get_mut(&key), value) {
                (Some(toml::Value::Table(inner)), toml::Value::Table(more)) => {
                    Self::merge_into(inner, more);
                }
                (_, value) => {
                    base.insert(key, value);
                }
            }
        }
    }

    /// The state file, where there is one.
    pub fn state_path(&self) -> Option<&Path> {
        self.apart.get(&Store::State)?.as_deref()
    }

    /// The data file, where there is one.
    pub fn data_path(&self) -> Option<&Path> {
        self.apart.get(&Store::Data)?.as_deref()
    }

    /// The system's file of what the user made in skwir
    /// (`$XDG_DATA_HOME/skwir/data.toml` on Linux): in `crate::folders::data`.
    pub fn default_data_path() -> Option<PathBuf> {
        crate::folders::data().map(|dir| dir.join("data.toml"))
    }

    /// The file apart from the settings `key` is kept in: a key declared
    /// for one, or one within it, where that store has a file. `None`: the
    /// settings file.
    fn apart_of(&self, kept: &BTreeMap<String, Store>, key: &str) -> Option<Store> {
        kept.iter()
            .find(|(declared, _)| {
                key == declared.as_str()
                    || key
                        .strip_prefix(declared.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
            .map(|(_, &store)| store)
            .filter(|store| self.apart.contains_key(store))
    }

    /// The keys kept in files apart from the settings, and which.
    fn kept_apart(&self) -> Vec<(String, Store)> {
        self.kept
            .read()
            .iter()
            .filter(|(_, store)| self.apart.contains_key(store))
            .map(|(key, &store)| (key.clone(), store))
            .collect()
    }

    /// Why the file was not read, once: the app says it to the user.
    pub fn take_problem(&self) -> Option<Words> {
        self.problem.lock().take()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether what changes in the file is taken in as it changes
    /// (`follow`): the watch is set up on a worker once the app starts.
    pub fn is_followed(&self) -> bool {
        !self.followed.lock().is_empty()
    }

    /// Declare a setting. Registered by plugins and the core. Refused while
    /// its owner (`SettingSpec::owner`) is off (`gated`): a worker of a
    /// plugin turned off that declares late (once it found what it needs)
    /// leaves no setting of a plugin that is off. The gate is read again
    /// under the schema's lock, which the withdrawal reads after closing it.
    pub fn declare(&self, mut spec: SettingSpec) {
        if !self.gate.is_open(spec.owner()) {
            return;
        }
        if spec.store != Store::Settings {
            self.kept.write().insert(spec.key.clone(), spec.store);
            self.read_apart(&spec.key, spec.store);
        }
        {
            let mut declared = self.declared.lock();
            let next = declared.len() as u64;
            spec.order = *declared.entry(spec.key.clone()).or_insert(next);
        }
        {
            let mut schema = self.schema.write();
            if !self.gate.is_open(spec.owner()) {
                return;
            }
            schema.insert(spec.key.clone(), spec);
        }
        self.bump();
    }

    /// Carry a value stored under a key's former name to its new one, when
    /// nothing is stored under the new name yet. The owner calls it before
    /// declaring the new key, so a moved preference keeps what the user
    /// chose; the file is rewritten at the next save, and until then the move
    /// is made again at each start, to the same effect. A file that still
    /// says the old name is told the new one (`check`).
    pub fn rename(&self, old: &str, new: &str) {
        self.renamed.lock().insert(old.to_owned(), new.to_owned());
        let mut pending = self.writer.lock();
        let mut values = self.values.write();
        if Self::lookup(&values, new).is_some() {
            return;
        }
        let Some(value) = Self::remove(&mut values, old) else {
            return;
        };
        if let Err(error) = Self::insert(&mut values, new, value) {
            tracing::warn!(%error, "cannot move a setting");
        }
        pending.extend([old.to_owned(), new.to_owned()]);
        drop(values);
        self.bump();
    }

    /// A key kept apart, as the file it is kept in holds it: whole, in
    /// place of a copy the settings file still has (an earlier skwir's, a
    /// hand's, Home Manager's), which is read only while that file has none.
    /// What this process changed and has not saved yet stays.
    fn read_apart(&self, key: &str, store: Store) {
        let pending = self.writer.lock();
        if pending.contains(key) {
            return;
        }
        let Some(value) = self
            .apart_read
            .lock()
            .get(&store)
            .and_then(|table| Self::lookup(table, key))
            .cloned()
        else {
            return;
        };
        let mut values = self.values.write();
        if Self::lookup(&values, key) == Some(&value) {
            return;
        }
        Self::remove(&mut values, key);
        if let Err(error) = Self::insert(&mut values, key, value) {
            tracing::warn!(%error, "cannot read a setting kept apart");
        }
        drop(values);
        drop(pending);
        self.bump();
    }

    /// Each key kept apart that `apart` holds, set in `table` whole: a copy
    /// `table` has (the settings file's) is never merged with it.
    fn replace_apart(
        table: &mut toml::Table,
        apart: &toml::Table,
        kept: impl IntoIterator<Item = String>,
    ) {
        for key in kept {
            if let Some(value) = Self::lookup(apart, &key) {
                Self::remove(table, &key);
                if let Err(error) = Self::insert(table, &key, value.clone()) {
                    tracing::warn!(%error, "cannot read a setting kept apart");
                }
            }
        }
    }

    /// Take a dotted key out, and the tables it leaves empty.
    fn remove(table: &mut toml::Table, key: &str) -> Option<toml::Value> {
        match key.split_once('.') {
            None => table.remove(key),
            Some((head, rest)) => {
                let inner = table.get_mut(head)?.as_table_mut()?;
                let value = Self::remove(inner, rest)?;
                if inner.is_empty() {
                    table.remove(head);
                }
                Some(value)
            }
        }
    }

    pub fn undeclare(&self, key: &str) {
        self.schema.write().remove(key);
        self.bump();
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    fn bump(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn spec(&self, key: &str) -> Option<SettingSpec> {
        self.schema.read().get(key).cloned()
    }

    pub fn schema(&self) -> Vec<SettingSpec> {
        self.schema.read().values().cloned().collect()
    }

    fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
        let mut parts = key.split('.');
        let mut value = table.get(parts.next()?)?;
        for part in parts {
            value = value.as_table()?.get(part)?;
        }
        Some(value)
    }

    /// Typed read: the stored value, else the declared default, else `None`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let values = self.values.read();
        let v = Self::lookup(&values, key)
            .cloned()
            .or_else(|| self.schema.read().get(key).map(|s| s.default.clone()))?;
        v.try_into().ok()
    }

    /// The value held for `key`, its declared default aside: `None` while
    /// nothing holds one (a choice never made, where `get` says the
    /// default).
    pub fn stored<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        Self::lookup(&self.values.read(), key)
            .cloned()?
            .try_into()
            .ok()
    }

    /// The live value as an editor shows it (strings without TOML quotes).
    pub fn text(&self, key: &str) -> Option<String> {
        self.get::<toml::Value>(key)
            .map(|value| SettingSpec::format_value(&value))
    }

    fn insert(table: &mut toml::Table, key: &str, value: toml::Value) -> anyhow::Result<()> {
        let parts: Vec<_> = key.split('.').collect();
        anyhow::ensure!(
            parts.iter().all(|p| !p.is_empty()),
            crate::words!(
                settings_invalid_key = "invalid setting key",
                note = "A setting's key with an empty part (a.b..c)."
            )
        );
        let mut table = table;
        for part in &parts[..parts.len() - 1] {
            table = table
                .entry((*part).to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(|| {
                    anyhow::Error::new(crate::words!(
                        settings_parent_not_table = "{ $key }: parent is not a table",
                        key = key,
                        note = "A setting's key whose part before it holds a value, not a table of settings."
                    ))
                })?;
        }
        table.insert(parts[parts.len() - 1].to_owned(), value);
        Ok(())
    }

    /// Drop a dotted key, so its default holds again and the file says it
    /// no more once saved.
    pub fn unset(&self, key: &str) {
        let state = self.is_state(key);
        let mut pending = self.writer.lock();
        Self::remove(&mut self.values.write(), key);
        pending.insert(key.to_owned());
        if !state {
            self.bump();
        }
    }

    /// Set a dotted key, creating intermediate tables. Writers run off the UI
    /// thread; use `settings.apply` for user preference changes.
    pub fn set(&self, key: &str, value: toml::Value) {
        let state = self.is_state(key);
        let mut pending = self.writer.lock();
        match Self::insert(&mut self.values.write(), key, value) {
            Ok(()) => {
                pending.insert(key.to_owned());
            }
            Err(error) => tracing::warn!(%error, "cannot set setting"),
        }
        if !state {
            self.bump();
        }
    }

    /// Whether `key` is what skwir keeps of the window as it was left: the
    /// state file (`revision` says what that means for it).
    fn is_state(&self, key: &str) -> bool {
        self.kept.read().get(key) == Some(&Store::State)
    }

    /// What is wrong in `table`, a settings file's or one to be (Home
    /// Manager's declared settings): each value checked against its
    /// setting (`SettingSpec::check`), a plugin's `enabled` against the
    /// plugins `plugins` names. A key no setting has, under an owner that
    /// declares settings (a typo, most likely), is a problem; one under an
    /// owner nothing here declares (a plugin turned off, or from outside)
    /// is only noted. A key kept apart from the settings (the window's
    /// state, the user's data) is noted: read only until skwir keeps its
    /// own. A key's former name (`rename`) is noted with its new one, or,
    /// where the file says the new one too, as not read.
    pub fn check(&self, table: &toml::Table, plugins: &[String]) -> Checked {
        let renamed = self.renamed.lock().clone();
        let moved = |key: &str| {
            renamed.get(key).map(|new| {
                if Self::lookup(table, new).is_some() {
                    crate::words!(
                        settings_renamed_both = "not read: { $new } is set, which Skwir reads",
                        new = crate::words::Name(new.clone()),
                        note = "A settings file's key that has a new name, set too: the new key."
                    )
                } else {
                    crate::words!(
                        settings_renamed = "now { $new }: Skwir moves it there",
                        new = crate::words::Name(new.clone()),
                        note = "A settings file's key that has a new name: the new key."
                    )
                }
            })
        };
        let schema = self.schema.read();
        let owners: BTreeSet<&str> = schema.values().map(SettingSpec::owner).collect();
        let mut checked = Checked::default();
        let mut walk = vec![(String::new(), table)];
        while let Some((prefix, table)) = walk.pop() {
            for (name, value) in table {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if let Some(note) = moved(&key) {
                    checked.notes.push((key, note));
                    continue;
                }
                if let Some(spec) = schema.get(&key) {
                    // Kept apart, it is read from the settings only until
                    // skwir keeps its own, which then replaces it whole: a
                    // note, never a problem the file would carry forever.
                    if spec.store != Store::Settings {
                        let file = spec.store.file_key();
                        let own = self
                            .apart_read
                            .lock()
                            .get(&spec.store)
                            .is_some_and(|kept| Self::lookup(kept, &key).is_some());
                        let note = if own {
                            crate::words!(
                                settings_apart_own = "{ $file ->
                                    [state] the window's state, not a setting: Skwir reads its own, in its state file, never this one
                                   *[data] what you make in Skwir (saved searches, pinned tags, tags' colours), not a setting: Skwir reads its own, in its data file, never this one
                                }",
                                file = file,
                                note = "A settings file's key that Skwir keeps in a file of its own, which holds it already."
                            )
                        } else {
                            crate::words!(
                                settings_apart_until = "{ $file ->
                                    [state] the window's state, not a setting: read from here only until Skwir keeps its own, in its state file
                                   *[data] what you make in Skwir (saved searches, pinned tags, tags' colours), not a setting: read from here only until Skwir keeps its own, in its data file
                                }",
                                file = file,
                                note = "A settings file's key that Skwir keeps in a file of its own, which does not hold it yet."
                            )
                        };
                        checked.notes.push((key, note));
                        continue;
                    }
                    let other = !spec.constraints.more_choices.is_empty()
                        && value.as_str().is_some_and(|value| {
                            !spec
                                .constraints
                                .choices
                                .iter()
                                .any(|choice| choice == value)
                        });
                    if other {
                        checked.notes.push((
                            key,
                            crate::words!(
                                settings_other_choice = "not one Skwir knows here: it may be among { $which }",
                                which = spec.constraints.more_choices.clone(),
                                note = "A settings file's choice Skwir does not know, which may be one found where it runs: which those are (themes you added to bat)."
                            ),
                        ));
                    } else if let Err(error) = spec.check(value) {
                        checked.problems.push((key, Words::of_error(error)));
                    }
                    continue;
                }
                if let Some(id) = key
                    .strip_prefix("plugins.")
                    .filter(|id| !id.contains('.'))
                    .map(str::to_owned)
                {
                    match value.as_table() {
                        Some(plugin) if plugins.contains(&id) => {
                            for (field, value) in plugin {
                                match (field.as_str(), value) {
                                    ("enabled", toml::Value::Boolean(_)) => {}
                                    ("enabled", _) => checked.problems.push((
                                        format!("{key}.enabled"),
                                        SettingSpec::expected_boolean(),
                                    )),
                                    _ => checked
                                        .problems
                                        .push((format!("{key}.{field}"), no_such_setting())),
                                }
                            }
                        }
                        Some(_) => {
                            let enabled = format!("{key}.enabled");
                            checked.notes.push(match moved(&enabled) {
                                Some(note) => (enabled, note),
                                None => (
                                    key,
                                    crate::words!(
                                        settings_no_plugin = "no plugin { $id } here: one from outside, or not built in, may take it",
                                        id = crate::words::Name(id.clone()),
                                        note = "A settings file names a plugin Skwir does not have: its id."
                                    ),
                                ),
                            });
                        }
                        None => checked.problems.push((
                            key,
                            crate::words!(
                                settings_plugin_table = "expected a table: enabled = true or false",
                                note = "A plugin's entry in a settings file; enabled, true and false are as the file writes them."
                            ),
                        )),
                    }
                    continue;
                }
                let within = format!("{key}.");
                if let Some(inner) = value.as_table()
                    && (key == "plugins"
                        || schema.keys().any(|declared| declared.starts_with(&within))
                        || renamed.keys().any(|old| old.starts_with(&within)))
                {
                    walk.push((key, inner));
                    continue;
                }
                if owners.contains(owner_of(&key)) {
                    checked.problems.push((key, no_such_setting()));
                } else {
                    checked.notes.push((
                        key.clone(),
                        crate::words!(
                            settings_owner_unknown = "nothing here declares { $owner }: a plugin turned off, or from outside, may",
                            owner = crate::words::Name(owner_of(&key).to_owned()),
                            note = "A settings file's key under an owner nothing here declares: the owner's id."
                        ),
                    ));
                }
            }
        }
        // By key, then as English says them: the same order wherever they
        // are said.
        checked
            .problems
            .sort_by_cached_key(|(key, said)| (key.clone(), said.english()));
        checked
            .notes
            .sort_by_cached_key(|(key, said)| (key.clone(), said.english()));
        checked
    }

    pub fn save_status(&self) -> (u64, SaveState) {
        self.status.lock().clone()
    }

    /// `changes` with each change of one key of a table a page draws
    /// (`exclusions.entries.<id>`, whose id may hold dots: that key's value
    /// as JSON, nothing to take it out) made a change of the whole table,
    /// that key over what the table holds now: what Home Manager's merge
    /// does, for a caller of `settings.apply`.
    ///
    /// # Errors
    /// When the value of one is no JSON.
    fn tables_whole(
        &self,
        changes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let table_of = |key: &str, schema: &BTreeMap<String, SettingSpec>| {
            key.match_indices('.')
                .map(|(at, _)| at)
                .find(|at| {
                    schema
                        .get(&key[..*at])
                        .is_some_and(|spec| spec.page_drawn && spec.default.is_table())
                })
                .map(|at| key[..at].to_owned())
        };
        let ones: Vec<(String, String)> = {
            let schema = self.schema.read();
            changes
                .keys()
                .filter(|key| !schema.contains_key(*key))
                .filter_map(|key| Some((key.clone(), table_of(key, &schema)?)))
                .collect()
        };
        if ones.is_empty() {
            return Ok(changes.clone());
        }
        let mut whole = changes.clone();
        for (key, table) in ones {
            let text = whole.remove(&key).unwrap_or_default();
            let id = &key[table.len() + 1..];
            let mut held: serde_json::Map<String, serde_json::Value> = match whole.get(&table) {
                Some(given) => {
                    serde_json::from_str(given).map_err(|error| Self::not_json(&table, &error))?
                }
                None => match Self::lookup(&self.values.read(), &table) {
                    Some(value) => serde_json::to_value(value)?
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                    None => serde_json::Map::new(),
                },
            };
            if text.trim().is_empty() {
                held.remove(id);
            } else {
                let value =
                    serde_json::from_str(&text).map_err(|error| Self::not_json(&key, &error))?;
                held.insert(id.to_owned(), value);
            }
            whole.insert(table, serde_json::Value::Object(held).to_string());
        }
        Ok(whole)
    }

    /// Why `key`'s value, given as JSON, could not be read.
    fn not_json(key: &str, error: &serde_json::Error) -> anyhow::Error {
        anyhow::Error::new(crate::words!(
            settings_key_not_json = "{ $key }: expected JSON: { $error }",
            key = key,
            error = error.to_string(),
            note = "A setting's key, then the JSON reader's own words."
        ))
    }

    pub fn validate(
        &self,
        changes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, toml::Value>> {
        use anyhow::Context as _;
        let changes = self.tables_whole(changes)?;
        // What each holds now, read before the schema's lock is taken.
        let stored: BTreeMap<&String, Option<toml::Value>> = {
            let values = self.values.read();
            changes
                .keys()
                .map(|key| (key, Self::lookup(&values, key).cloned()))
                .collect()
        };
        let schema = self.schema.read();
        changes
            .iter()
            .map(|(key, text)| {
                let spec = schema.get(key).ok_or_else(|| {
                    anyhow::Error::new(crate::words!(
                        settings_unknown = "unknown setting: { $key }",
                        key = key.as_str(),
                        note = "A setting's key no setting has."
                    ))
                })?;
                let value = spec
                    .parse_over(text, stored.get(key).and_then(Option::as_ref))
                    .with_context(|| key.clone())?;
                Ok((key.clone(), value))
            })
            .collect()
    }

    /// What the file `key` is kept apart in (the state, the user's data)
    /// holds for it as it stands now, read from the disk: another skwir's
    /// may be newer than this process's, which a save writes over whole.
    /// `None` where `key` is kept in no file apart, or the file holds none
    /// or cannot be read. Reads the disk: on a worker.
    pub fn kept_on_disk(&self, key: &str) -> Option<toml::Value> {
        let store = self.apart_of(&self.kept.read(), key)?;
        let path = self.apart.get(&store)?.as_ref()?;
        Self::lookup(&Self::on_disk(path).ok()?, key).cloned()
    }

    /// The file as it stands: empty when there is none.
    ///
    /// # Errors
    /// When it cannot be read, or is no TOML (a hand edit under way).
    fn on_disk(path: &Path) -> anyhow::Result<toml::Table> {
        let bytes = match crate::fs::file::read_regular(path, FILE_LIMIT) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(toml::Table::new()),
            Err(e) => anyhow::bail!(crate::words!(
                settings_file_unreadable = "{ $path } could not be read: { $why }",
                path = crate::words::Arg::Path(path.display().to_string()),
                why = crate::op::io_words(&e),
                note = "A file of the settings: its path, then why it could not be read."
            )),
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            anyhow::Error::new(crate::words!(
                settings_file_not_text = "{ $path } is not text",
                path = crate::words::Arg::Path(path.display().to_string())
            ))
        })?;
        text.parse::<toml::Table>().map_err(|e| {
            anyhow::Error::new(crate::words!(
                settings_file_invalid = "{ $path } is not valid ({ $problem }): nothing is saved over it until it is fixed",
                path = crate::words::Arg::Path(path.display().to_string()),
                problem = toml_problem(text, &e),
                note = "A file of the settings that is no TOML (a hand edit under way): its path, and what is wrong where."
            ))
        })
    }

    /// `base` with the `keys` of `values` over it: those `values` holds
    /// set, the others taken out.
    fn overlay(
        mut base: toml::Table,
        values: &toml::Table,
        keys: &BTreeSet<String>,
    ) -> anyhow::Result<toml::Table> {
        for key in keys {
            match Self::lookup(values, key) {
                Some(value) => Self::insert(&mut base, key, value.clone())?,
                None => {
                    Self::remove(&mut base, key);
                }
            }
        }
        Ok(base)
    }

    /// Write `table` to `path` whole, put in place at once: through a link
    /// to where it leads (a settings file kept with the user's other
    /// configuration), never over the link.
    ///
    /// # Errors
    /// When it cannot be written; a file that cannot be (a link into a
    /// store, a file made read-only) says it is read-only.
    fn write_whole(path: &Path, store: Store, table: &toml::Table) -> anyhow::Result<()> {
        let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink()) {
            std::fs::canonicalize(path)?
        } else {
            path.to_path_buf()
        };
        if Self::is_read_only(&target) {
            anyhow::bail!(Self::read_only_words(store, path, &target));
        }
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                    anyhow::anyhow!(Self::read_only_words(store, path, &target))
                }
                _ => anyhow::Error::new(error),
            })?;
        file.write_all(toml::to_string_pretty(table)?.as_bytes())?;
        file.as_file().sync_all()?;
        crate::fs::file::take_place(file, &target)?;
        // The rename is durable only once the directory entry is
        // flushed, on Unix; Windows journals it, and opens no folder so.
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    /// Whether the file at `path` (where a link leads) is there and
    /// cannot be written.
    fn is_read_only(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly())
    }

    fn read_only_words(store: Store, path: &Path, target: &Path) -> Words {
        let file = store.file_key();
        if target == path {
            crate::words!(
                settings_read_only = "the { $file ->
                    [state] state file
                    [data] data file
                   *[settings] settings file
                } is read-only, so the change is not kept",
                file = file
            )
        } else if store == Store::Settings {
            crate::words!(
                settings_read_only_link = "the settings file is a link to one that cannot be written (Home Manager's, say), so the change is not kept: change it where it is declared",
                note = "Home Manager is a program that declares a user's configuration."
            )
        } else {
            crate::words!(
                settings_read_only_link_apart = "the { $file ->
                    [state] state file
                   *[data] data file
                } is a link to one that cannot be written, so the change is not kept",
                file = file
            )
        }
    }

    /// Whether the settings cannot be saved because their file is
    /// read-only (a link into the Nix store, as Home Manager makes it): the
    /// dialog says so, and changes nothing.
    pub fn read_only(&self) -> bool {
        self.path.as_deref().is_some_and(Self::is_read_only)
    }

    /// Write the `keys` of `values` over the files as they stand, keeping
    /// what another process or a hand changed there: those kept apart in
    /// their files first (the state, then the user's data), the others in
    /// the settings file. A file none of whose keys changed is not touched
    /// (a read-only settings file never is by a change of the window's
    /// state or of a saved search), nor needs to be read: one that is no
    /// TOML (a hand edit under way) stops only a change of its own keys,
    /// after the others are saved. What is saved: the settings file's
    /// table with the others' (`values` itself where there is no file, or
    /// it cannot be read and none of its keys changed).
    fn persist(
        &self,
        values: &toml::Table,
        keys: &BTreeSet<String>,
    ) -> anyhow::Result<toml::Table> {
        let kept = self.kept.read().clone();
        let mut apart_keys: BTreeMap<Store, BTreeSet<String>> = BTreeMap::new();
        let mut settings = BTreeSet::new();
        for key in keys {
            match self.apart_of(&kept, key) {
                Some(store) => {
                    apart_keys.entry(store).or_default().insert(key.clone());
                }
                None => {
                    settings.insert(key.clone());
                }
            }
        }
        // What this process holds of the keys kept in `store`.
        let held = |store: Store| -> anyhow::Result<toml::Table> {
            let mut table = toml::Table::new();
            for (key, _) in kept.iter().filter(|(_, s)| **s == store) {
                if let Some(value) = Self::lookup(values, key) {
                    Self::insert(&mut table, key, value.clone())?;
                }
            }
            Ok(table)
        };
        let mut apart = toml::Table::new();
        for (&store, path) in &self.apart {
            let (Some(keys), Some(path)) = (apart_keys.get(&store), path) else {
                // Nothing of it changed, or it is saved nowhere: as this
                // process holds it.
                Self::merge_into(&mut apart, held(store)?);
                continue;
            };
            let (disk, read) = match store {
                // The user's: never written over when it cannot be read.
                Store::Data => (Self::on_disk(path)?, true),
                // skwir's own: one that is no TOML (cut short, an edit gone
                // wrong) is started over from all this process holds of
                // it, not only what it changed, so neither the file nor
                // this process loses the rest.
                _ => match Self::on_disk(path) {
                    Ok(disk) => (disk, true),
                    Err(error) => {
                        tracing::warn!("started over: {error:#}");
                        (held(store)?, false)
                    }
                },
            };
            let merged = Self::overlay(disk.clone(), values, keys)?;
            if !read || merged != disk {
                Self::write_whole(path, store, &merged)?;
            }
            self.apart_read.lock().insert(store, merged.clone());
            Self::merge_into(&mut apart, merged);
        }
        let mut saved = match &self.path {
            Some(path) => match Self::on_disk(path) {
                Ok(disk) => {
                    let merged = Self::overlay(disk.clone(), values, &settings)?;
                    if merged != disk {
                        Self::write_whole(path, Store::Settings, &merged)?;
                    }
                    merged
                }
                // None of its keys changed: nothing of it is taken in
                // until it can be read again (`reload`).
                Err(error) if settings.is_empty() => {
                    tracing::debug!("{error:#}");
                    values.clone()
                }
                Err(error) => return Err(error),
            },
            None => values.clone(),
        };
        Self::merge_into(&mut saved, apart.clone());
        // A key kept apart is its file's, whole: never merged with a copy
        // the settings file still holds.
        let declared = kept
            .into_iter()
            .filter(|(_, store)| self.apart.contains_key(store))
            .map(|(key, _)| key);
        Self::replace_apart(&mut saved, &apart, declared);
        Ok(saved)
    }

    /// A transaction: failed validation or persistence leaves live values intact.
    /// Only changed keys are merged; unrelated settings survive an open editor.
    fn apply(&self, changes: &BTreeMap<String, String>) -> anyhow::Result<Vec<String>> {
        let mut pending = self.writer.lock();
        let parsed = self.validate(changes)?;
        let mut values = self.values.read().clone();
        let mut keys = Vec::new();
        for (key, value) in parsed {
            if self.get::<toml::Value>(&key).as_ref() != Some(&value) {
                Self::insert(&mut values, &key, value)?;
                keys.push(key);
            }
        }
        if !keys.is_empty() {
            let mut written = pending.clone();
            written.extend(keys.iter().cloned());
            let saved = self.persist(&values, &written)?;
            pending.clear();
            *self.values.write() = saved;
            self.bump();
        }
        Ok(keys)
    }

    /// Take in what changed in the files since this process read or saved
    /// them (another skwir, a hand edit, Home Manager): the settings file
    /// and the data file as they stand, with what this process changed and
    /// has not saved yet over them. The declared keys whose value changed,
    /// and the plugins' `enabled`; `None` when there is no settings file to
    /// follow. A settings file that cannot be read is left as it is (the
    /// next save of one of its keys says why), and the data file is taken
    /// in all the same.
    pub fn reload(&self) -> Option<Vec<String>> {
        let path = self.path.as_ref()?;
        let pending = self.writer.lock();
        let values = self.values.read().clone();
        let mut merged = match Self::on_disk(path) {
            Ok(disk) => match Self::overlay(disk, &values, &pending) {
                Ok(merged) => merged,
                Err(error) => {
                    tracing::warn!("the settings file cannot take this window's: {error:#}");
                    values.clone()
                }
            },
            Err(error) => {
                tracing::warn!("{error:#}");
                values.clone()
            }
        };
        // What is kept apart is its own file's, not the settings file's:
        // the state as this process holds it, the user's data as its file
        // stands (another skwir, `skwir cmd`), with what this process
        // changed over it.
        let data = self.data_path().and_then(|path| match Self::on_disk(path) {
            Ok(disk) => {
                self.apart_read.lock().insert(Store::Data, disk.clone());
                Self::overlay(disk, &values, &pending).ok()
            }
            Err(error) => {
                tracing::warn!("{error:#}");
                None
            }
        });
        for (key, store) in self.kept_apart() {
            let from = match (store, &data) {
                (Store::Data, Some(data)) => data,
                _ => &values,
            };
            // Whole, over the settings file's copy; with none of its own,
            // the settings file's is read until skwir keeps one.
            if let Some(value) = Self::lookup(from, &key) {
                Self::remove(&mut merged, &key);
                // A parent that is not a table is refused, and the value
                // stays out: what the settings file says wins.
                let _ = Self::insert(&mut merged, &key, value.clone());
            }
        }
        if merged == values {
            return Some(Vec::new());
        }
        let differs = |key: &str| Self::lookup(&values, key) != Self::lookup(&merged, key);
        let mut keys: Vec<String> = self
            .schema
            .read()
            .keys()
            .filter(|key| differs(key))
            .cloned()
            .collect();
        let plugins = |table: &toml::Table| {
            table
                .get("plugins")
                .and_then(toml::Value::as_table)
                .map(|plugins| plugins.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let mut ids = plugins(&values);
        ids.extend(plugins(&merged));
        ids.sort_unstable();
        ids.dedup();
        keys.extend(
            ids.into_iter()
                .map(|id| format!("plugins.{id}.enabled"))
                .filter(|key| differs(key)),
        );
        *self.values.write() = merged;
        drop(pending);
        self.bump();
        Some(keys)
    }

    /// Persist to the file, if any: what this process changed, over the
    /// file as it stands, whose other changes it takes in. Call from a
    /// worker, never during rendering.
    pub fn save(&self) -> anyhow::Result<()> {
        let mut pending = self.writer.lock();
        let values = self.values.read().clone();
        let saved = match self.persist(&values, &pending) {
            Ok(saved) => saved,
            // A change a read-only file cannot take is undone, once: the
            // settings are what the file says (what is kept apart was saved
            // first).
            Err(error) if self.read_only() => {
                pending.clear();
                if let Some(path) = &self.path
                    && let Ok(disk) = Self::on_disk(path)
                {
                    let mut back = disk;
                    for (key, _) in self.kept_apart() {
                        Self::remove(&mut back, &key);
                        if let Some(value) = Self::lookup(&values, &key) {
                            let _ = Self::insert(&mut back, &key, value.clone());
                        }
                    }
                    *self.values.write() = back;
                    self.bump();
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        pending.clear();
        if saved != values {
            *self.values.write() = saved;
            self.bump();
        }
        Ok(())
    }
}

/// The title of `core.preserve_attributes`, which a copy's refusal names
/// as the setting to turn off: one message, said in both (ADR-139, 8).
pub(crate) fn keep_attributes_title() -> Words {
    crate::words!(
        settings_keep_attributes = "Keep tags and attributes when copying",
        note = "A setting on the Tags and notes page; a copy that cannot keep them names it too."
    )
}

#[cfg(test)]
mod under_tests {
    use super::{SettingSpec, register};
    use crate::words::Words;

    /// Under a switch, a setting applies while it is on; under a choice,
    /// with the values it names alone; declared again under a switch, it
    /// names none.
    #[test]
    fn a_setting_applies_with_what_it_is_under() {
        let switch =
            SettingSpec::toggle("a.more", Words::as_written("More"), "Does.", true).under("a.on");
        assert!(switch.applies_with("true"));
        assert!(!switch.applies_with("false"));
        let chosen = SettingSpec::text("a.folder", Words::as_written("Folder"), "Does.", "")
            .under_choice("a.start", &["folder", "here"]);
        assert_eq!(chosen.under, "a.start");
        assert!(chosen.applies_with("folder"));
        assert!(chosen.applies_with("here"));
        assert!(!chosen.applies_with("last"));
        assert!(!chosen.applies_with("true"));
        let again = chosen.under("a.on");
        assert!(again.under_values.is_empty());
        assert!(again.applies_with("true"));
    }

    /// The startup folder is read only when a chosen folder opens at
    /// startup, and is shown so.
    #[test]
    fn the_startup_folder_is_under_a_chosen_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        register(&app);
        let folder = app
            .settings
            .schema()
            .into_iter()
            .find(|spec| spec.key == super::STARTUP_FOLDER)
            .unwrap();
        assert_eq!(folder.under, super::STARTUP);
        assert!(folder.applies_with("folder"));
        assert!(!folder.applies_with("last"));
    }
}

#[cfg(test)]
mod stored_tests {
    use super::{SettingSpec, Settings};
    use crate::words::Words;

    /// What is stored is told from the default: nothing until a value is
    /// set, whatever the default, and nothing again once unset.
    #[test]
    fn a_stored_value_is_told_from_the_default() {
        let settings = Settings::in_memory();
        assert_eq!(settings.stored::<bool>("a.on"), None);
        settings.declare(SettingSpec::toggle(
            "a.on",
            Words::as_written("On"),
            "Does.",
            false,
        ));
        assert_eq!(settings.get::<bool>("a.on"), Some(false), "the default");
        assert_eq!(settings.stored::<bool>("a.on"), None, "not stored");
        settings.set("a.on", toml::Value::Boolean(false));
        assert_eq!(settings.stored::<bool>("a.on"), Some(false));
        settings.unset("a.on");
        assert_eq!(settings.stored::<bool>("a.on"), None);
    }
}

#[cfg(test)]
mod gate_tests {
    use super::SettingSpec;
    use crate::words::Words;

    /// A plugin turned off declares nothing: a declaration made while it is
    /// off (a worker of it, late) is refused, its other keys and the core's
    /// declared still; turned on again, it declares.
    #[test]
    fn a_plugin_turned_off_declares_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let toggle = |key: &str| {
            SettingSpec::toggle(key, Words::as_written("On".to_owned()), "Does.", false)
        };
        app.registry.set_owner_enabled("late", false);
        app.settings.declare(toggle("late.tell"));
        assert!(
            app.settings.spec("late.tell").is_none(),
            "refused while off"
        );
        app.settings.declare(toggle("other.tell"));
        assert!(app.settings.spec("other.tell").is_some());
        app.registry.set_owner_enabled("late", true);
        app.settings.declare(toggle("late.tell"));
        assert!(
            app.settings.spec("late.tell").is_some(),
            "on again, declared"
        );
    }
}

#[cfg(test)]
mod kept_tests {
    use super::{ChoiceSource, SettingKind, SettingSpec};
    use crate::words::Words;

    /// A choice its owner keeps is listed by the owner's command, asked
    /// again as the owner says it moved, and never applied as a
    /// preference.
    #[test]
    fn a_kept_choice_is_its_owners() {
        let source = ChoiceSource::new("owner.choice");
        let spec = SettingSpec::choice_kept_by(
            "owner.kept",
            Words::as_written("Kept"),
            "Chosen elsewhere.",
            source.clone(),
        );
        assert_eq!(spec.kind, SettingKind::KeptChoice);
        let before = spec.kept_source().unwrap().1;
        assert_eq!(spec.kept_source().unwrap().0, "owner.choice");
        source.changed();
        assert_ne!(
            spec.kept_source().unwrap().1,
            before,
            "the dialog lists it again"
        );
        assert!(spec.parse_over("anything", None).is_err());

        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        app.settings.declare(spec);
        let changes = std::collections::BTreeMap::from([("owner.kept".to_owned(), "x".to_owned())]);
        assert!(app.settings.validate(&changes).is_err());
    }
}

#[cfg(test)]
mod language_tests {
    use crate::words::{Offer, Share, Shipped};

    /// The Language setting lists a language Skwir offers by its own name,
    /// one Automatic speaks as a draft as one, and one it does not as
    /// incomplete, with its share in whole percents rounded down (ADR-139,
    /// 13).
    #[test]
    fn a_language_not_offered_is_listed_as_incomplete_with_its_share() {
        static SHIPPED: [Shipped; 4] = [
            Shipped {
                domain: "skwir-core",
                language: "fr",
                source: "settings_language_self = Français\n",
            },
            Shipped {
                domain: "skwir-core",
                language: "de",
                source: "settings_language_self = Deutsch\n",
            },
            Shipped {
                domain: "skwir-core",
                language: "es",
                source: "settings_language_self = Español\n",
            },
            Shipped {
                domain: "skwir-core",
                language: "it",
                source: "settings_language_self = Italiano\n",
            },
        ];
        static SHARES: [Share; 3] = [
            Share {
                language: "fr",
                messages: 1000,
                translated: 999,
                reviewed: 9,
                offered: Offer::Incomplete,
                name: "Français",
            },
            Share {
                language: "de",
                messages: 1000,
                translated: 950,
                reviewed: 920,
                offered: Offer::Reviewed,
                name: "Deutsch",
            },
            Share {
                language: "it",
                messages: 1000,
                translated: 1000,
                reviewed: 129,
                offered: Offer::Draft,
                name: "Italiano",
            },
        ];
        crate::words::ship(&SHIPPED);
        crate::words::ship_shares(&SHARES);
        crate::words::register_shipped_of_program("skwir");
        let said: Vec<(&str, String)> = super::language_choices()
            .into_iter()
            .map(|(value, label)| (value, label.english()))
            .collect();
        let mut wanted = vec![
            ("automatic", "Automatic".to_owned()),
            ("en", "English".to_owned()),
            (
                "fr",
                "Français — incomplete, 99% translated, 0% reviewed".to_owned(),
            ),
            ("de", "Deutsch".to_owned()),
            // Shipped but never counted: offered no more than one short.
            (
                "es",
                "Español — incomplete, 0% translated, 0% reviewed".to_owned(),
            ),
            // Spoken by Automatic before it is reviewed.
            ("it", "Italiano — draft, 12% reviewed".to_owned()),
        ];
        if cfg!(debug_assertions) {
            wanted.push((crate::words::PSEUDO, "Pseudo (for testing)".to_owned()));
            wanted.push((
                crate::direction::PSEUDO_RTL,
                "Pseudo, right to left (for testing)".to_owned(),
            ));
        }
        assert_eq!(said, wanted);
        // Named from the shares `just words` counted: no counted
        // language's catalog read to list it; one shipped uncounted is
        // said from its catalog.
        for counted in ["fr", "de", "it"] {
            assert!(
                !crate::words::was_read("skwir-core", counted),
                "{counted}'s catalog read to name it"
            );
        }
        assert!(crate::words::was_read("skwir-core", "es"));
        crate::words::withdraw("skwir");
        crate::words::ship_shares(&[]);
        crate::words::ship(&[]);
    }
}

#[cfg(test)]
mod on_disk_tests {
    use super::{SettingSpec, Settings};
    use crate::words::Words;

    /// What another skwir wrote in the state file is read from it, while
    /// this one's own copy stays as it read it; a key kept in no file
    /// apart is read from none.
    #[test]
    fn a_key_kept_apart_is_read_as_its_file_stands() {
        let tmp = tempfile::tempdir().unwrap();
        let open = || {
            let settings = Settings::load(tmp.path().join("settings.toml"))
                .with_state(tmp.path().join("state.toml"));
            settings.declare(SettingSpec::state(
                "a.kept",
                Words::as_written("What a test keeps."),
                toml::Value::Table(toml::Table::new()),
            ));
            settings
        };
        let (one, other) = (open(), open());
        assert_eq!(one.kept_on_disk("a.kept"), None, "nothing written yet");
        let value = toml::Value::Integer(2);
        other.set("a.kept", value.clone());
        other.save().unwrap();
        assert_eq!(one.kept_on_disk("a.kept"), Some(value));
        assert_eq!(one.stored::<i64>("a.kept"), None, "its own as it read it");
        one.set("a.setting", toml::Value::Integer(1));
        one.save().unwrap();
        assert_eq!(one.kept_on_disk("a.setting"), None, "the settings file's");
    }
}
