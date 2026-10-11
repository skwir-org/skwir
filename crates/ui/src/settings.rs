//! The Settings dialog: a self-contained view over the core's settings schema,
//! plugin host and key hints. It holds no reference to the shell, so it can be
//! hosted in an overlay or in a window of its own.
//!
//! Changes apply immediately. Switches and choices submit on click; text and
//! number fields submit on Enter or when they lose focus. Every change goes
//! through `settings.apply`, which saves atomically on a worker; while one save
//! is in flight later changes wait in `queue` and leave as one batch. A failed
//! save restores the live value next to the control and says why.
//!
//! Rendering lives in `settings_view.rs`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Window,
};
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::plugin::{PluginInfo, RequirementStatus};
use skwir_core::settings::{SaveState, SettingKind, SettingSpec};
use std::sync::Mutex;

use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::keymap::KeyHint;

/// Settings namespaces that are not plugins, each a page of the dialog:
/// what skwir does with files, how it looks, and how the preview reads.
/// A preference goes where its subject is: the plugin whose content it
/// concerns has it on its page (an engine, the player of audio and video);
/// one the shell applies to several plugins' content is a namespace of the
/// shell's own, as `reading` is for the reader and reading mode, and the
/// colors code is drawn in, shared by documents, books, comics, Markdown
/// and the code view. `ui` is only how skwir looks.
pub const CORE_OWNER: &str = "core";
pub const UI_OWNER: &str = "ui";
pub const READING_OWNER: &str = "reading";
/// The stores notes are kept in (ADR-027), shared by several plugins.
pub const NOTES_OWNER: &str = skwir_core::notes::SETTINGS;
/// What leaves skwir (ADR-044): the private folders.
pub const PRIVACY_OWNER: &str = skwir_core::permissions::SETTINGS;

pub const THEME_KEY: &str = "ui.theme";
/// The icon theme skwir draws with, by its folder's name; Automatic is the
/// desktop's (`icons::IconCache::set_theme`). The free desktops' only:
/// macOS and Windows draw the system's own icons.
pub const ICON_THEME_KEY: &str = "ui.icon_theme";
/// `ui.icon_theme`'s Automatic.
pub const ICON_THEME_AUTO: &str = "auto";
pub const DATES_KEY: &str = "ui.date_format";
/// Whether the grid crops thumbnails to fill their box.
pub const GRID_FILL_KEY: &str = "ui.grid_fill";
/// Whether a folder has the Narrow by bar above its files (`narrow.rs`).
pub const NARROW_FOLDERS_KEY: &str = "ui.narrow_folders";
/// Whether the tab strip shows for a window that has only one tab.
pub const SINGLE_TAB_KEY: &str = "ui.single_tab";
/// Whether a folder's trail starts at the root rather than at the
/// sidebar's folder that holds it (`Shell::sync_roots`).
pub const FULL_PATH_KEY: &str = "ui.full_path";
/// How documents are read: one scrolling column, or a page at a time.
pub const READER_CONTINUOUS_KEY: &str = "reading.continuous";
/// How wide a reading column may grow (`markdown::measure`).
pub const READING_WIDTH_KEY: &str = "reading.width";
/// Whether a book's position counts its printed pages (`ui.printed_pages`).
pub const PRINTED_PAGES_KEY: &str = "reading.printed_pages";
/// Whether a text's long lines wrap to the panel (`ui.wrap_lines`).
pub const WRAP_LINES_KEY: &str = "reading.wrap_lines";
/// Whether code is drawn light or dark: Automatic is as skwir is
/// (`code_preview::CodeTheme::chosen`).
pub const CODE_APPEARANCE_KEY: &str = "reading.code_appearance";
/// The colors code is drawn in when it is light, and when it is dark
/// (`code_preview::CodeTheme`).
pub const CODE_LIGHT_KEY: &str = "reading.code_light";
pub const CODE_DARK_KEY: &str = "reading.code_dark";
/// The player's preferences. The audio and video plugin (`preview-media`)
/// declares them, with the rest of what concerns playback; the shell's
/// player reads them. With the plugin off there is no player, and they are
/// gone with it. `crates/skwir/tests/settings.rs` holds the two to the
/// same names and choices.
/// Whether a track that starts to play keeps playing while you browse.
pub const KEEP_PLAYING_KEY: &str = "preview-media.keep_playing";
/// Whether audio and video start to play as soon as they are selected.
pub const AUTOPLAY_KEY: &str = "preview-media.autoplay";
/// What follows a track that ends (`player::Continue`).
pub const CONTINUE_KEY: &str = "preview-media.continue";
/// What the player does with nothing stored, as the plugin must declare it.
pub const KEEP_PLAYING_DEFAULT: bool = true;
pub const AUTOPLAY_DEFAULT: bool = false;
/// The values of `CONTINUE_KEY`, the first the default.
pub const CONTINUE_CHOICES: [&str; 3] = [
    crate::player::Continue::Stop.key(),
    crate::player::Continue::Next.key(),
    crate::player::Continue::Shuffle.key(),
];

/// The icon theme setting: Automatic, then the themes installed, by name,
/// once listed (`list_icon_themes`: it reads the icon folders, a worker's
/// work), whose value is the theme's folder (`breeze`). The one chosen is
/// always a choice, said missing once the list shows it is not installed.
pub fn icon_theme_spec(
    chosen: &str,
    themes: Option<&[skwir_platform::icons::Theme]>,
) -> SettingSpec {
    let mut choices = vec![(ICON_THEME_AUTO.to_owned(), "Automatic".to_owned())];
    if let Some(themes) = themes {
        choices.extend(themes.iter().map(|t| (t.folder.clone(), t.name.clone())));
    }
    if !chosen.is_empty() && !choices.iter().any(|(value, _)| value == chosen) {
        let label = if themes.is_some() {
            format!("{chosen} (not installed)")
        } else {
            chosen.to_owned()
        };
        choices.push((chosen.to_owned(), label));
    }
    let pairs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    SettingSpec::choice(
        ICON_THEME_KEY,
        "Icon theme",
        "The icons skwir draws files, folders and places with. Automatic follows your desktop's choice.",
        ICON_THEME_AUTO,
        &pairs,
    )
}

/// A look's code theme setting: skwir's own colors, bat's themes of that
/// look, then those the user added to bat once read (`list_code_themes`),
/// the one chosen always among them (`code_preview::choices`).
pub fn code_theme_spec(
    dark: bool,
    chosen: &str,
    user: Option<&[Arc<crate::code_preview::UserTheme>]>,
) -> SettingSpec {
    let choices = crate::code_preview::choices(dark, chosen, user);
    let pairs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    let (key, title, look, choice) = if dark {
        (CODE_DARK_KEY, "Dark theme", "dark", "Dark")
    } else {
        (CODE_LIGHT_KEY, "Light theme", "light", "Light")
    };
    SettingSpec::choice(
        key,
        title,
        &format!(
            "The colors of source files and of code in documents when code is {look}: while skwir is {look}, or always with Appearance set to {choice}. Default is skwir's own, on its {look} background; a theme brings its own background, bold and italic. Themes you added to bat, in its themes folder, follow bat's own."
        ),
        crate::code_preview::DEFAULT,
        &pairs,
    )
    .section("Code")
}

/// Read the themes the user added to bat on a worker and declare the code
/// theme settings again with them: at start, and when the settings open,
/// where a theme added or edited since is read.
pub(crate) fn list_code_themes(app: &Arc<App>) {
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "code-themes",
        app.lifetime().child(),
        move || {
            let user = crate::code_preview::user_themes(true);
            let Some(app) = weak.upgrade() else { return };
            for (dark, key) in [(false, CODE_LIGHT_KEY), (true, CODE_DARK_KEY)] {
                let chosen = app.settings.get::<String>(key).unwrap_or_default();
                app.settings
                    .declare(code_theme_spec(dark, &chosen, Some(&user)));
            }
            app.wake();
        },
    );
}

/// List the installed icon themes on a worker and declare the icon theme
/// setting again with them: when the settings open, where the list is
/// read. Nothing where the system draws its own icons.
pub(crate) fn list_icon_themes(app: &Arc<App>) {
    if skwir_platform::system_icons::AVAILABLE {
        return;
    }
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "icon-themes",
        app.lifetime().child(),
        move || {
            let themes = skwir_platform::icons::installed_themes();
            let Some(app) = weak.upgrade() else { return };
            let chosen = app
                .settings
                .get::<String>(ICON_THEME_KEY)
                .unwrap_or_default();
            app.settings
                .declare(icon_theme_spec(&chosen, Some(&themes)));
            app.wake();
        },
    );
}

/// Declare the shell's own preferences. They live in the shell's namespaces
/// (`ui`, `reading`) and exist only while a shell does, so headless tools
/// never see them.
pub fn declare(app: &App) {
    // Where they were before they had a page of their own.
    for (old, new) in [
        ("ui.reader_continuous", READER_CONTINUOUS_KEY),
        ("ui.reading_width", READING_WIDTH_KEY),
        ("ui.printed_pages", PRINTED_PAGES_KEY),
    ] {
        app.settings.rename(old, new);
    }
    app.settings.declare(SettingSpec::choice(
        THEME_KEY,
        "Theme",
        "Follow the desktop's light or dark preference, or always use one.",
        "auto",
        &[("auto", "Automatic"), ("light", "Light"), ("dark", "Dark")],
    ));
    if !skwir_platform::system_icons::AVAILABLE {
        let chosen = app
            .settings
            .get::<String>(ICON_THEME_KEY)
            .unwrap_or_default();
        app.settings.declare(icon_theme_spec(&chosen, None));
        // Before anything is drawn, as the shell will say it again.
        skwir_platform::icons::choose_theme(
            Some(chosen.as_str()).filter(|c| *c != ICON_THEME_AUTO),
        );
    }
    app.settings.declare(SettingSpec::choice(
        DATES_KEY,
        "Dates",
        "Friendly dates read as Today 14:02, Tuesday 09:30 or 12 Mar, with the exact time in a tooltip. ISO shows 2026-03-12 14:02 everywhere.",
        "friendly",
        &[("friendly", "Friendly"), ("iso", "ISO 8601")],
    ));
    app.settings.declare(
        SettingSpec::toggle(
            SINGLE_TAB_KEY,
            "Show a lone tab",
            "Show the tab strip even when the window has one tab, named like the place it shows. Off gives the lists that row; New tab stays in the More menu and on Ctrl+T.",
            false,
        )
        .section("Tabs"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            FULL_PATH_KEY,
            "Show the full path",
            "Show a folder's whole path above its files, from the root, each folder under its own name: / › home › someone › Documents › 2026. Off starts it at the deepest folder of the sidebar that holds it, under the sidebar's name: Documents › 2026, a drive's name for a folder on a drive.",
            false,
        )
        .section("Address"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            GRID_FILL_KEY,
            "Fill thumbnails",
            "Crop each thumbnail in the grid to a square, so a folder of pictures reads as a gallery. Off shows every picture whole.",
            false,
        )
        .section("Grid"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            NARROW_FOLDERS_KEY,
            "Narrow folders",
            "Show the Narrow by bar above a folder's files: their kinds, the years they were modified and their tags, each with how many files, to show only those. The choice stays while you browse the folders inside. A search and a tag's folder offer it whatever this says.",
            false,
        )
        .section("Folders"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            READER_CONTINUOUS_KEY,
            "Continuous scrolling",
            "Read a document, a book laid out in pages or a comic as one scrolling column. Off shows one page at a time, and the reader's own button still changes it for the document in front of you.",
            true,
        )
        .section("Pages"),
    );
    app.settings.declare(
        SettingSpec::choice(
            READING_WIDTH_KEY,
            "Reading width",
            "How long the lines of a book, a Markdown or HTML file may grow before the text stops widening and sits in the middle of the preview: about 65 characters for Narrow, 85 for Comfortable, 110 for Wide. Full width uses the whole panel, with small margins.",
            "comfortable",
            &[
                ("narrow", "Narrow"),
                ("comfortable", "Comfortable"),
                ("wide", "Wide"),
                ("full", "Full width"),
            ],
        )
        .section("Text"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            PRINTED_PAGES_KEY,
            "Printed page numbers",
            "Count where you are in a book by the page numbers of its printed edition, when it lists them: page 47 of 312 rather than chapter 3 of 12, and in a PDF by the numbers its pages are printed with. A book without them counts chapters.",
            false,
        )
        .section("Pages"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            WRAP_LINES_KEY,
            "Wrap lines",
            "Wrap a text file's long lines to the panel, a line's number on its first row. Off, they go on sideways and the panel scrolls with them.",
            true,
        )
        .section("Code"),
    );
    app.settings.declare(
        SettingSpec::choice(
            CODE_APPEARANCE_KEY,
            "Appearance",
            "Draw source files and code in documents light or dark. Automatic follows skwir's theme; Light or Dark keeps code in the theme chosen below for that look, with its gutter, selection and background, whatever skwir is.",
            "auto",
            &[("auto", "Automatic"), ("light", "Light"), ("dark", "Dark")],
        )
        .section("Code"),
    );
    for (dark, key) in [(false, CODE_LIGHT_KEY), (true, CODE_DARK_KEY)] {
        let chosen = app.settings.get::<String>(key).unwrap_or_default();
        app.settings.declare(code_theme_spec(dark, &chosen, None));
    }
    app.settings.declare(SettingSpec::data(
        crate::shortcuts::HINTS_KEY,
        "Commands whose shortcut hint was already shown",
        toml::Value::Array(Vec::new()),
    ));
    // The list's detail columns, in the order they stand (`COLUMNS`).
    for column in &crate::views::list::COLUMNS {
        app.settings.declare(
            SettingSpec::toggle(
                column.setting,
                column.label,
                column.description,
                column.shown,
            )
            .section("List columns"),
        );
    }
}

/// Save a preference changed where it is used (the reader's button, the
/// player's menu) rather than in the dialog. It goes the dialog's way,
/// through `settings.apply`, which saves on a worker and tells everyone
/// else. The core takes one save at a time: while another runs, the change
/// waits (`flush_remembered`, on every wake) instead of failing, and
/// `preference` reads it meanwhile.
pub fn remember(
    shell: &mut crate::shell::Shell,
    key: &'static str,
    value: String,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    shell.unsaved.insert(key, value);
    flush_remembered(shell, cx);
}

/// Send the waiting changes as one batch, unless a save is running.
pub fn flush_remembered(
    shell: &mut crate::shell::Shell,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    if shell.unsaved.is_empty() || shell.app.settings.save_status().1 == SaveState::Saving {
        return;
    }
    let changes: serde_json::Map<String, serde_json::Value> = std::mem::take(&mut shell.unsaved)
        .into_iter()
        .filter(|(key, value)| shell.app.settings.text(key).as_ref() != Some(value))
        .map(|(key, value)| (key.to_owned(), value.into()))
        .collect();
    if changes.is_empty() {
        return;
    }
    if let Err(error) = shell.app.dispatch(&Invocation::with_args(
        "settings.apply",
        &serde_json::json!({ "changes": changes }),
    )) {
        shell.set_status(format!("{error:#}"), true, cx);
    }
}

/// A preference as it stands: the change waiting to be saved, else the
/// stored value, as text.
pub fn preference(shell: &crate::shell::Shell, key: &str) -> Option<String> {
    shell
        .unsaved
        .get(key)
        .cloned()
        .or_else(|| shell.app.settings.text(key))
}

/// Record how the user chose to read documents: the reader's button is the
/// same choice as the one in the dialog.
pub fn remember_reader_mode(
    shell: &mut crate::shell::Shell,
    continuous: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    remember(shell, READER_CONTINUOUS_KEY, continuous.to_string(), cx);
}

/// How the next document opens: the choice waiting to be saved, else the
/// stored one.
pub fn reader_mode(shell: &crate::shell::Shell) -> bool {
    preference(shell, READER_CONTINUOUS_KEY).is_none_or(|mode| mode != "false")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Appearance,
    Reading,
    Notes,
    Privacy,
    Plugins,
    Keyboard,
    Plugin(String),
}

/// One of the dialog's own pages: the word `ui.settings` names it by, the
/// namespace whose settings it shows (none for Plugins), its title, glyph
/// and introduction. Every reading of a page (by owner, by name, its
/// title, glyph and words, the sidebar's order) is `BUILTIN`'s.
pub(crate) struct Builtin {
    page: Page,
    name: &'static str,
    owner: Option<&'static str>,
    title: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) intro: &'static str,
    /// Listed only while a setting is on it.
    when_used: bool,
}

/// The dialog's own pages, in the sidebar's order; plugins' follow.
static BUILTIN: [Builtin; 7] = [
    Builtin {
        page: Page::General,
        name: "general",
        owner: Some(CORE_OWNER),
        title: "General",
        icon: "sliders",
        intro: "How skwir handles files. Changes apply as you make them.",
        when_used: false,
    },
    Builtin {
        page: Page::Appearance,
        name: "appearance",
        owner: Some(UI_OWNER),
        title: "Appearance",
        icon: "appearance",
        intro: "How skwir looks.",
        when_used: false,
    },
    Builtin {
        page: Page::Reading,
        name: "reading",
        owner: Some(READING_OWNER),
        title: "Reading",
        icon: "reading",
        intro: "How documents, books, comics, Markdown and code read in the preview. Which engine reads each is on its plugin's page.",
        when_used: false,
    },
    Builtin {
        page: Page::Notes,
        name: "notes",
        owner: Some(NOTES_OWNER),
        title: "Notes",
        icon: "tag",
        intro: "Where tags, ratings and comments are kept, so the desktop, a Mac and other applications see them too. Every place is read and merged; each is written as you choose: never, where the file already has notes there, also where a file's volume keeps no attributes (one place is started there, the one its folder already uses first), or for every file edited.",
        // Only while a plugin keeps notes somewhere.
        when_used: true,
    },
    Builtin {
        page: Page::Privacy,
        name: "privacy",
        owner: Some(PRIVACY_OWNER),
        title: "Privacy",
        icon: "lock",
        intro: "What leaves this machine. An agent driving skwir, and a plugin from outside skwir, never see a private folder: not its names, not what its files hold. What each plugin may do is said on its line of the Plugins page.",
        when_used: false,
    },
    Builtin {
        page: Page::Plugins,
        name: "plugins",
        owner: None,
        title: "Plugins",
        icon: "plugin",
        intro: "Everything beyond the core is a plugin. Turning one off removes its commands, views and settings until it is turned on again.",
        when_used: false,
    },
    Builtin {
        page: Page::Keyboard,
        name: "keyboard",
        owner: Some(skwir_core::keymap::OWNER),
        title: "Keyboard",
        icon: "keyboard",
        intro: "Your shortcuts, then every shortcut in use, grouped by where it applies. Yours replace the others on the same keys.",
        when_used: false,
    },
];

impl Page {
    pub fn of_owner(owner: &str) -> Page {
        BUILTIN
            .iter()
            .find(|b| b.owner == Some(owner))
            .map_or_else(|| Page::Plugin(owner.to_owned()), |b| b.page.clone())
    }

    /// A page by its word, or by the namespace it shows (`core` is
    /// General), else the plugin of that id.
    fn parse(name: &str) -> Page {
        BUILTIN
            .iter()
            .find(|b| b.name == name || b.owner == Some(name))
            .map_or_else(|| Page::Plugin(name.to_owned()), |b| b.page.clone())
    }

    /// The dialog's own page this is; `None` for a plugin's.
    pub(crate) fn builtin(&self) -> Option<&'static Builtin> {
        BUILTIN.iter().find(|b| &b.page == self)
    }
}

pub enum SettingsEvent {
    Close,
}

pub(crate) struct Row {
    pub spec: SettingSpec,
    /// What the control shows: the live value, or an optimistic one whose
    /// save is queued or in flight.
    pub value: String,
    /// Present for kinds edited as text.
    pub input: Option<Entity<TextInput>>,
    pub anchor: PopoverAnchor,
    _subscriptions: Vec<Subscription>,
}

impl Row {
    pub fn default_text(&self) -> String {
        SettingSpec::format_value(&self.spec.default)
    }

    /// A preference changed from its default. A list of records is data
    /// the user built: no Revert, and Reset all leaves it alone.
    pub fn modified(&self) -> bool {
        self.spec.kind != SettingKind::Records && self.value != self.default_text()
    }

    pub fn page(&self) -> Page {
        Page::of_owner(self.spec.owner())
    }
}

/// One keyboard stop of the current frame, in traversal order.
pub(crate) struct Stop {
    pub focus: FocusHandle,
    /// Child index inside the scrolling page, to bring it into view.
    pub item: Option<usize>,
    /// The setting a left/right adjustment applies to.
    pub key: Option<String>,
}

pub struct SettingsView {
    pub(crate) app: Arc<App>,
    pub(crate) font: SharedString,
    pub(crate) key_hints: Vec<KeyHint>,
    /// Each of `key_hints`' place and command as the Keyboard page says
    /// them, worked out when they are set: asked for every row of every
    /// frame, a command's title was a search of the shell's commands.
    pub(crate) key_words: Vec<(String, String)>,
    /// The Keyboard page's rows for a query, worked out again when the
    /// query or the bindings change, not at each frame.
    pub(crate) key_rows:
        std::cell::RefCell<Option<(String, std::rc::Rc<Vec<crate::settings_view::KeyRow>>)>>,
    /// The Keyboard page's list of bindings, which draws those in view.
    pub(crate) key_list: gpui::UniformListScrollHandle,
    pub(crate) page: Page,
    pub(crate) search: Entity<TextInput>,
    pub(crate) rows: Vec<Row>,
    pub(crate) plugins: Vec<PluginInfo>,
    /// `None` until the first probe finishes.
    pub(crate) requirements: Option<Vec<RequirementStatus>>,
    probe: Arc<Mutex<Option<Vec<RequirementStatus>>>>,
    pub(crate) root: FocusHandle,
    pub(crate) nav: FocusHandle,
    handles: HashMap<String, FocusHandle>,
    pub(crate) stops: Vec<Stop>,
    /// Child index of each setting row in the current frame.
    pub(crate) row_items: BTreeMap<String, usize>,
    pub(crate) scroll: gpui::ScrollHandle,
    pub(crate) errors: BTreeMap<String, String>,
    queue: BTreeMap<String, String>,
    inflight: Option<(u64, BTreeMap<String, String>)>,
    pub(crate) saved_flash: bool,
    flash_generation: u64,
    settings_revision: u64,
    plugins_revision: u64,
    /// Plugins whose toggle was requested and has not been observed yet.
    pub(crate) toggling: BTreeMap<String, bool>,
    pub(crate) reset_armed: bool,
    pub(crate) dropdown: Option<String>,
    /// Setting to point out after `ui.settings {key}`.
    pub(crate) highlight: Option<String>,
    scroll_to: Option<String>,
    /// The record edited, per records setting (`settings_records`).
    pub(crate) drafts: BTreeMap<String, crate::settings_records::Draft>,
    /// The focus handles of each record's buttons this frame, with the
    /// setting and the record's id: what Alt+Up and Alt+Down move.
    pub(crate) record_stops: Vec<(FocusHandle, String, String)>,
    /// What keyring jobs came back with, for `sync`.
    pub(crate) keyring: Arc<Mutex<Vec<crate::settings_records::KeyringDone>>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

/// What the Keyboard page says of each binding: its place and its command.
fn key_words(app: &App, hints: &[KeyHint]) -> Vec<(String, String)> {
    hints
        .iter()
        .map(|hint| {
            (
                crate::keymap::context_label(&hint.context),
                crate::keymap::title_of(app, hint),
            )
        })
        .collect()
}

impl SettingsView {
    pub fn new(
        app: Arc<App>,
        font: SharedString,
        key_hints: Vec<KeyHint>,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search settings", cx));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    this.scroll.set_offset(gpui::Point::default());
                    cx.notify();
                }
                InputEvent::Cancel => {
                    if input.read(cx).text().is_empty() {
                        cx.emit(SettingsEvent::Close);
                    } else {
                        input.update(cx, |input, cx| input.set_text("", cx));
                        cx.notify();
                    }
                }
                InputEvent::Submit => this.step_focus(1, window, cx),
            },
        )];
        let mut view = SettingsView {
            plugins: app.plugins.list(),
            settings_revision: app.settings.revision(),
            plugins_revision: app.plugins.revision(),
            key_words: key_words(&app, &key_hints),
            key_rows: std::cell::RefCell::default(),
            key_list: gpui::UniformListScrollHandle::new(),
            app,
            font,
            key_hints,
            page: Page::General,
            search,
            rows: Vec::new(),
            requirements: None,
            probe: Arc::new(Mutex::new(None)),
            drafts: BTreeMap::new(),
            record_stops: Vec::new(),
            keyring: Arc::default(),
            root: cx.focus_handle(),
            nav: cx.focus_handle(),
            handles: HashMap::new(),
            stops: Vec::new(),
            row_items: BTreeMap::new(),
            scroll: gpui::ScrollHandle::new(),
            errors: BTreeMap::new(),
            queue: BTreeMap::new(),
            inflight: None,
            saved_flash: false,
            flash_generation: 0,
            toggling: BTreeMap::new(),
            reset_armed: false,
            dropdown: None,
            highlight: None,
            scroll_to: None,
            _subscriptions: subscriptions,
        };
        view.rebuild(window, cx);
        view.probe_requirements();
        list_icon_themes(&view.app);
        list_code_themes(&view.app);
        window.focus(&view.nav);
        view.open_target(target, window, cx);
        view
    }

    /// A stable focus handle for a control that has no state of its own.
    pub(crate) fn handle(&mut self, id: &str, cx: &mut Context<Self>) -> FocusHandle {
        if let Some(handle) = self.handles.get(id) {
            return handle.clone();
        }
        let handle = cx.focus_handle();
        self.handles.insert(id.to_owned(), handle.clone());
        handle
    }

    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        self.search.read(cx).text().trim().to_lowercase()
    }

    pub(crate) fn plugin_title(&self, id: &str) -> String {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .map_or_else(|| id.to_owned(), |plugin| plugin.title.clone())
    }

    /// The keymap changed while the dialog is open (a shortcut saved here,
    /// a plugin turned on or off).
    pub fn set_key_hints(&mut self, hints: Vec<KeyHint>, cx: &mut Context<Self>) {
        self.key_words = key_words(&self.app, &hints);
        self.key_hints = hints;
        self.key_rows.take();
        cx.notify();
    }

    pub(crate) fn page_title(&self, page: &Page) -> String {
        match (page.builtin(), page) {
            (Some(builtin), _) => builtin.title.into(),
            (None, Page::Plugin(id)) => self.plugin_title(id),
            (None, _) => String::new(),
        }
    }

    /// Sidebar entries: the fixed pages, then every plugin that has settings.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let mut pages: Vec<Page> = BUILTIN
            .iter()
            .filter(|b| !b.when_used || self.rows.iter().any(|row| row.page() == b.page))
            .map(|b| b.page.clone())
            .collect();
        let mut owners: Vec<Page> = self
            .rows
            .iter()
            .map(Row::page)
            .filter(|page| matches!(page, Page::Plugin(_)))
            .collect();
        owners.dedup();
        owners.sort_by_key(|page| self.page_title(page).to_lowercase());
        pages.extend(owners);
        pages
    }

    // ------------------------------------------------------------ live state

    fn live(&self, spec: &SettingSpec) -> String {
        self.app
            .settings
            .text(&spec.key)
            .unwrap_or_else(|| SettingSpec::format_value(&spec.default))
    }

    fn pending(&self, key: &str) -> bool {
        self.queue.contains_key(key)
            || self
                .inflight
                .as_ref()
                .is_some_and(|(_, changes)| changes.contains_key(key))
    }

    /// Reconcile rows with the schema: plugins declare and withdraw settings
    /// as they are toggled, and another surface may have changed a value.
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut previous: HashMap<String, Row> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(|row| (row.spec.key.clone(), row))
            .collect();
        let mut specs: Vec<SettingSpec> = self
            .app
            .settings
            .schema()
            .into_iter()
            .filter(SettingSpec::editable)
            .collect();
        // Declaration order is alphabetical by key; group sections together
        // while keeping that order inside each.
        specs.sort_by(|a, b| (a.owner(), &a.section).cmp(&(b.owner(), &b.section)));
        for spec in specs {
            let live = self.live(&spec);
            let row = match previous.remove(&spec.key) {
                Some(mut row) if row.spec.kind == spec.kind => {
                    if !self.pending(&spec.key) {
                        row.value.clone_from(&live);
                        if let Some(input) = &row.input
                            && !input.read(cx).is_focused(window)
                        {
                            input.update(cx, |input, cx| input.set_text(live.clone(), cx));
                        }
                    }
                    row.spec = spec;
                    row
                }
                _ => self.new_row(spec, live, window, cx),
            };
            self.rows.push(row);
        }
        self.errors
            .retain(|key, _| self.rows.iter().any(|row| &row.spec.key == key));
    }

    fn new_row(
        &self,
        spec: SettingSpec,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Row {
        let textual = matches!(
            spec.kind,
            SettingKind::Integer
                | SettingKind::Number
                | SettingKind::Text
                | SettingKind::Path
                | SettingKind::Command
        );
        let mut subscriptions = Vec::new();
        let input = textual.then(|| {
            let placeholder = if spec.kind == SettingKind::Path {
                "Not set"
            } else {
                ""
            };
            let input = cx.new(|cx| TextInput::new(placeholder, cx));
            input.update(cx, |input, cx| input.set_text(value.clone(), cx));
            let key = spec.key.clone();
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, _, cx| match event {
                    InputEvent::Changed => {
                        this.errors.remove(&key);
                        this.highlight = None;
                        cx.notify();
                    }
                    InputEvent::Submit => this.commit(&key, cx),
                    InputEvent::Cancel => {
                        let shown = this.row(&key).map(|row| row.value.clone());
                        if shown.as_deref() == Some(input.read(cx).text()) {
                            cx.emit(SettingsEvent::Close);
                        } else if let Some(shown) = shown {
                            input.update(cx, |input, cx| input.set_text(shown, cx));
                            this.errors.remove(&key);
                            cx.notify();
                        }
                    }
                },
            ));
            let key = spec.key.clone();
            subscriptions.push(
                cx.on_blur(&input.focus_handle(cx), window, move |this, _, cx| {
                    this.commit(&key, cx);
                }),
            );
            input
        });
        Row {
            spec,
            value,
            input,
            anchor: PopoverAnchor::default(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn row(&self, key: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.spec.key == key)
    }

    /// Called by the host whenever the core woke the UI.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(id) = self.inflight.as_ref().map(|(id, _)| *id) {
            let (current, state) = self.app.settings.save_status();
            // The core keeps the latest save only, and ids only grow: another
            // id means ours ended and another surface saved since. How ours
            // went is unknown; the live values below say what holds.
            let ended = current != id || state != SaveState::Saving;
            if ended {
                let (_, sent) = self.inflight.take().expect("checked above");
                match state {
                    _ if current != id => {}
                    SaveState::Failed(error) => {
                        for key in sent.keys() {
                            self.errors.insert(key.clone(), error.clone());
                        }
                        self.saved_flash = false;
                    }
                    _ => self.flash(cx),
                }
                // Values of a failed save fall back to the live ones.
                self.settings_revision = u64::MAX;
                changed = true;
            }
        }
        let (settings, plugins) = (self.app.settings.revision(), self.app.plugins.revision());
        if plugins != self.plugins_revision {
            self.plugins_revision = plugins;
            self.plugins = self.app.plugins.list();
            let plugins = &self.plugins;
            self.toggling.retain(|id, wanted| {
                plugins
                    .iter()
                    .any(|plugin| plugin.id == id && plugin.enabled != *wanted)
            });
            changed = true;
        }
        if settings != self.settings_revision {
            self.settings_revision = settings;
            self.rebuild(window, cx);
            changed = true;
        }
        if let Some(found) = self.probe.lock().ok().and_then(|mut slot| slot.take()) {
            self.requirements = Some(found);
            changed = true;
        }
        changed |= self.drain_keyring(cx);
        self.flush(cx);
        if changed {
            cx.notify();
        }
    }

    fn flash(&mut self, cx: &mut Context<Self>) {
        self.saved_flash = true;
        self.flash_generation += 1;
        let generation = self.flash_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1800))
                .await;
            this.update(cx, |this, cx| {
                if this.flash_generation == generation {
                    this.saved_flash = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn saving(&self) -> bool {
        self.inflight.is_some() || !self.queue.is_empty()
    }

    // ------------------------------------------------------------ changes

    /// Validate and submit one change. Invalid text stays in its field with
    /// the reason below it; nothing is sent.
    pub(crate) fn change(&mut self, key: &str, text: String, cx: &mut Context<Self>) {
        self.highlight = None;
        self.reset_armed = false;
        let Some(row) = self.rows.iter_mut().find(|row| row.spec.key == key) else {
            return;
        };
        if let Err(error) = row.spec.parse(&text) {
            self.errors.insert(key.to_owned(), format!("{error:#}"));
            cx.notify();
            return;
        }
        self.errors.remove(key);
        if let Some(input) = &row.input
            && input.read(cx).text() != text
        {
            input.update(cx, |input, cx| input.set_text(text.clone(), cx));
        }
        if row.value == text {
            cx.notify();
            return;
        }
        row.value.clone_from(&text);
        self.queue.insert(key.to_owned(), text);
        self.flush(cx);
        cx.notify();
    }

    fn commit(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(text) = self
            .row(key)
            .and_then(|row| row.input.as_ref())
            .map(|input| input.read(cx).text().to_owned())
        else {
            return;
        };
        if self.row(key).is_some_and(|row| row.value != text) {
            self.change(key, text, cx);
        }
    }

    /// Submit the queue unless a save is running; `sync` calls again when it
    /// ends. Another surface (CLI, agent) may hold the save: we wait for it
    /// the same way instead of reporting an error.
    fn flush(&mut self, cx: &mut Context<Self>) {
        if self.inflight.is_some()
            || self.queue.is_empty()
            || self.app.settings.save_status().1 == SaveState::Saving
        {
            return;
        }
        let changes = std::mem::take(&mut self.queue);
        match self.app.commands.dispatch(
            &Invocation::with_args("settings.apply", &serde_json::json!({ "changes": changes })),
            &self.app,
        ) {
            Ok(Outcome::Value(id)) => self.inflight = Some((id.as_u64().unwrap_or(0), changes)),
            Ok(_) => {}
            Err(error) => {
                for key in changes.keys() {
                    self.errors.insert(key.clone(), format!("{error:#}"));
                }
                self.settings_revision = u64::MAX;
            }
        }
        cx.notify();
    }

    /// Left/right on a focused choice or number.
    fn adjust(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self
            .stops
            .iter()
            .find(|stop| stop.focus.contains_focused(window, cx))
            .and_then(|stop| stop.key.clone())
        else {
            return;
        };
        self.adjust_key(&key, delta, cx);
    }

    pub(crate) fn adjust_key(&mut self, key: &str, delta: i64, cx: &mut Context<Self>) {
        let Some(row) = self.row(key) else { return };
        let text = match row.spec.kind {
            SettingKind::Toggle => (delta > 0).to_string(),
            SettingKind::Choice => {
                let choices = &row.spec.constraints.choices;
                let Some(len) = i64::try_from(choices.len()).ok().filter(|len| *len > 0) else {
                    return;
                };
                let current = choices
                    .iter()
                    .position(|choice| *choice == row.value)
                    .and_then(|index| i64::try_from(index).ok())
                    .unwrap_or(0);
                let next = (current + delta).clamp(0, len - 1);
                choices[usize::try_from(next).unwrap_or(0)].clone()
            }
            SettingKind::Integer => {
                let shown = row
                    .input
                    .as_ref()
                    .and_then(|input| input.read(cx).text().trim().parse::<i64>().ok())
                    .or_else(|| row.value.parse().ok())
                    .or_else(|| row.spec.default.as_integer())
                    .unwrap_or(0);
                let (min, max) = row
                    .spec
                    .constraints
                    .integer_range
                    .unwrap_or((i64::MIN, i64::MAX));
                shown
                    .saturating_add(delta.saturating_mul(row.spec.constraints.step.max(1)))
                    .clamp(min, max)
                    .to_string()
            }
            SettingKind::Number => {
                let shown = row
                    .input
                    .as_ref()
                    .and_then(|input| input.read(cx).text().trim().parse::<f64>().ok())
                    .or_else(|| row.value.parse().ok())
                    .or_else(|| row.spec.default.as_float())
                    .unwrap_or(0.);
                let step = row.spec.constraints.step.max(1) as f64;
                SettingSpec::format_value(&toml::Value::Float(shown + delta as f64 * step))
            }
            _ => return,
        };
        self.change(key, text, cx);
    }

    pub(crate) fn reset(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(text) = self.row(key).map(Row::default_text) {
            self.change(key, text, cx);
        }
    }

    pub(crate) fn reset_all(&mut self, cx: &mut Context<Self>) {
        if !self.reset_armed {
            self.reset_armed = true;
            cx.notify();
            return;
        }
        let keys: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.modified())
            .map(|row| row.spec.key.clone())
            .collect();
        for key in keys {
            self.reset(&key, cx);
        }
        self.reset_armed = false;
    }

    pub(crate) fn browse(&mut self, key: &str, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        let key = key.to_owned();
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.first() {
                        this.change(&key, path.to_string_lossy().into_owned(), cx);
                    }
                }
                Ok(Err(error)) => {
                    this.errors.insert(key.clone(), format!("{error:#}"));
                    cx.notify();
                }
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn set_plugin(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        match self.app.commands.dispatch(
            &Invocation::with_args(
                "plugins.set_enabled",
                &serde_json::json!({ "id": id, "enabled": enabled }),
            ),
            &self.app,
        ) {
            Ok(_) => {
                self.toggling.insert(id.to_owned(), enabled);
            }
            Err(error) => self
                .app
                .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}")),
        }
        cx.notify();
    }

    pub(crate) fn probe_requirements(&mut self) {
        let slot = Arc::clone(&self.probe);
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "settings-requirements",
            self.app.lifetime().child(),
            move || {
                let Some(app) = weak.upgrade() else { return };
                let found = app.plugins.probe_requirements();
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(found);
                }
                app.wake();
            },
        );
    }

    /// Open the settings file in the user's editor. The file exists only
    /// once something differs from the defaults.
    pub(crate) fn open_file(&self) {
        let Some(path) = self.app.settings.path().map(std::path::Path::to_path_buf) else {
            return;
        };
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "settings-open-file",
            self.app.lifetime().child(),
            move || {
                use skwir_core::app::NotifyLevel;
                let Some(app) = weak.upgrade() else { return };
                if !path.is_file() {
                    app.notify(
                        NotifyLevel::Info,
                        "There is no settings file yet: every setting has its default value",
                    );
                } else if let Err(error) = skwir_platform::open::open_path(&path) {
                    app.notify(NotifyLevel::Error, format!("{error:#}"));
                }
            },
        );
    }

    // ------------------------------------------------------------ navigation

    pub(crate) fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| input.set_text("", cx));
        self.page = page;
        self.dropdown = None;
        self.reset_armed = false;
        self.scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    /// `{ "key": "plugin.option" }` or `{ "page": "plugins" }`.
    pub fn open_target(
        &mut self,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |name: &str| target.get(name).and_then(serde_json::Value::as_str);
        if let Some(key) = text("key") {
            let owner = key.split('.').next().unwrap_or_default();
            self.go(Page::of_owner(owner), cx);
            if self.row(key).is_some() {
                self.highlight = Some(key.to_owned());
                self.scroll_to = Some(key.to_owned());
            }
        } else if let Some(page) = text("page") {
            self.go(Page::parse(page), cx);
        }
        window.focus(&self.nav);
    }

    /// The row to scroll to once it has been laid out, consumed by render.
    pub(crate) fn take_scroll_target(&mut self) -> Option<String> {
        self.scroll_to.take()
    }

    fn step_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let pages = self.pages();
        let current = pages
            .iter()
            .position(|page| *page == self.page)
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(pages.len().saturating_sub(1));
        self.go(pages[next].clone(), cx);
    }

    fn step_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.stops.is_empty() {
            return;
        }
        let len = self.stops.len();
        let next = match self
            .stops
            .iter()
            .position(|stop| stop.focus.contains_focused(window, cx))
        {
            Some(current) => (current + len).saturating_add_signed(delta) % len,
            None if delta > 0 => 0,
            None => len - 1,
        };
        let stop = &self.stops[next];
        if let Some(item) = stop.item {
            self.scroll.scroll_to_item(item);
        }
        window.focus(&stop.focus);
        cx.notify();
    }

    /// `ui.settings_*` commands, from key bindings and other surfaces.
    pub fn command(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = args
            .get("delta")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(1);
        match name {
            "settings_close" => {
                if self.dropdown.take().is_some() {
                    cx.notify();
                } else {
                    cx.emit(SettingsEvent::Close);
                }
            }
            "settings_next" => self.step_focus(1, window, cx),
            "settings_prev" => self.step_focus(-1, window, cx),
            "settings_search" => {
                self.search.update(cx, |input, cx| {
                    input.select_all_now(cx);
                    input.focus(window);
                });
            }
            "settings_page" => self.step_page(isize::try_from(delta).unwrap_or(1), cx),
            "settings_enter" => {
                // From the category list into the page: the stop after it.
                window.focus(&self.nav);
                self.step_focus(1, window, cx);
            }
            "settings_adjust" => self.adjust(delta, window, cx),
            "settings_move" => self.move_focused_record(delta, window, cx),
            _ => {}
        }
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.root.clone()
    }
}

// ------------------------------------------------------------ hosting

impl crate::shell::Shell {
    /// Open the dialog, or point the open one at `{ key }` / `{ page }`.
    pub fn open_settings(
        &mut self,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = &self.settings {
            settings.update(cx, |settings, cx| settings.open_target(target, window, cx));
            return;
        }
        self.palette = None;
        self.menu = None;
        if let Some(popup) = self.toolbar_menu.take() {
            popup.restore(window);
        }
        let (app, font, hints) = (
            Arc::clone(&self.app),
            self.font.clone(),
            self.key_hints.to_vec(),
        );
        // What had the keyboard gets it back when the dialog closes, as a
        // popup's does: the reader, the address, the pane.
        let mut back = Some(crate::modal::FocusReturn::capture(window, cx));
        let settings = cx.new(|cx| SettingsView::new(app, font, hints, target, window, cx));
        self.settings_subscription = Some(cx.subscribe_in(
            &settings,
            window,
            move |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Close => {
                    this.settings = None;
                    this.settings_subscription = None;
                    let fallback = this.ui().focus.clone();
                    back.take().unwrap_or_default().restore(window, &fallback);
                    cx.notify();
                }
            },
        ));
        self.settings = Some(settings);
    }

    pub fn settings_command(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.command(name, args, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ICON_THEME_AUTO, Page, icon_theme_spec};
    use skwir_platform::icons::Theme;

    #[test]
    fn the_icon_theme_is_automatic_or_one_installed_by_its_folder() {
        let themes = [
            Theme {
                folder: "Adwaita".into(),
                name: "Adwaita".into(),
            },
            Theme {
                folder: "breeze".into(),
                name: "Breeze".into(),
            },
        ];
        let listed = icon_theme_spec(ICON_THEME_AUTO, Some(&themes));
        assert_eq!(listed.constraints.choices, ["auto", "Adwaita", "breeze"]);
        assert_eq!(
            listed.constraints.choice_labels,
            ["Automatic", "Adwaita", "Breeze"]
        );
        assert_eq!(listed.default.as_str(), Some("auto"));
        // Chosen and listed: no more than its name.
        let chosen = icon_theme_spec("breeze", Some(&themes));
        assert_eq!(chosen.constraints.choices.len(), 3);
        // Chosen, not listed yet: a choice under its folder's name.
        let early = icon_theme_spec("Papirus", None);
        assert_eq!(early.constraints.choices, ["auto", "Papirus"]);
        assert_eq!(early.constraints.choice_labels, ["Automatic", "Papirus"]);
        // Chosen and no longer installed: said so.
        let gone = icon_theme_spec("Papirus", Some(&themes));
        assert_eq!(
            gone.constraints.choices.last().map(String::as_str),
            Some("Papirus")
        );
        assert_eq!(
            gone.constraints.choice_labels.last().map(String::as_str),
            Some("Papirus (not installed)")
        );
    }

    #[test]
    fn a_page_is_named_by_its_word_or_its_namespace() {
        assert_eq!(Page::parse("general"), Page::General);
        assert_eq!(Page::parse("core"), Page::General);
        assert_eq!(Page::parse("ui"), Page::Appearance);
        assert_eq!(Page::parse("plugins"), Page::Plugins);
        assert_eq!(Page::parse("tags"), Page::Plugin("tags".into()));
        assert_eq!(Page::of_owner(skwir_core::keymap::OWNER), Page::Keyboard);
        assert!(Page::Plugin("tags".into()).builtin().is_none());
    }
}
