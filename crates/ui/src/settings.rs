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

use bibli_core::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::plugin::{PluginInfo, RequirementStatus};
use bibli_core::settings::{SaveState, SettingKind, SettingSpec};
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Window,
};
use std::sync::Mutex;

use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::keymap::KeyHint;

/// Settings namespaces that are not plugins.
pub const CORE_OWNER: &str = "core";
pub const UI_OWNER: &str = "ui";

pub const THEME_KEY: &str = "ui.theme";
pub const DATES_KEY: &str = "ui.date_format";
/// Widths of the detail columns, set by dragging their header edges.
pub const COLUMN_WIDTHS_KEY: &str = "ui.column_widths";
/// Detail columns of the list view, in the order they give way when a pane
/// gets narrow (the last one first).
pub const COLUMN_KEYS: [&str; 3] = ["ui.column_size", "ui.column_modified", "ui.column_type"];
/// Whether the grid crops thumbnails to fill their box.
pub const GRID_FILL_KEY: &str = "ui.grid_fill";
/// How documents are read: one scrolling column, or a page at a time.
pub const READER_CONTINUOUS_KEY: &str = "ui.reader_continuous";
/// How wide a reading column may grow (`markdown::measure`).
pub const READING_WIDTH_KEY: &str = "ui.reading_width";
/// Whether a book's position counts its printed pages (`ui.printed_pages`).
pub const PRINTED_PAGES_KEY: &str = "ui.printed_pages";
/// Whether a track that starts to play keeps playing while you browse.
pub const KEEP_PLAYING_KEY: &str = "ui.media_keep_playing";
/// Whether music and films start to play as soon as they are selected.
pub const AUTOPLAY_KEY: &str = "ui.media_autoplay";
/// What follows a track that ends (`player::Continue`).
pub const CONTINUE_KEY: &str = "ui.media_continue";

/// Declare the shell's own preferences. They live in the `ui` namespace and
/// exist only while a shell does, so headless tools never see them.
pub fn declare(app: &App) {
    app.settings.declare(SettingSpec::choice(
        THEME_KEY,
        "Theme",
        "Follow the desktop's light or dark preference, or always use one.",
        "auto",
        &[("auto", "Automatic"), ("light", "Light"), ("dark", "Dark")],
    ));
    app.settings.declare(SettingSpec::choice(
        DATES_KEY,
        "Dates",
        "Friendly dates read as Today 14:02, Tuesday 09:30 or 12 Mar, with the exact time in a tooltip. ISO shows 2026-03-12 14:02 everywhere.",
        "friendly",
        &[("friendly", "Friendly"), ("iso", "ISO 8601")],
    ));
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
            READER_CONTINUOUS_KEY,
            "Continuous scrolling",
            "Read a document as one scrolling column. Off shows one page at a time, and the reader's own button still changes it for the document in front of you.",
            true,
        )
        .section("Documents"),
    );
    app.settings.declare(
        SettingSpec::choice(
            READING_WIDTH_KEY,
            "Reading width",
            "How long the lines of a book or a Markdown file may grow before the text stops widening and sits in the middle of the preview: about 65 characters for Narrow, 85 for Comfortable, 110 for Wide. Full width uses the whole panel, with small margins.",
            "comfortable",
            &[
                ("narrow", "Narrow"),
                ("comfortable", "Comfortable"),
                ("wide", "Wide"),
                ("full", "Full width"),
            ],
        )
        .section("Documents"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            PRINTED_PAGES_KEY,
            "Printed page numbers",
            "Count where you are in a book by the page numbers of its printed edition, when it lists them: page 47 of 312 rather than chapter 3 of 12. A book without them counts chapters.",
            false,
        )
        .section("Documents"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            KEEP_PLAYING_KEY,
            "Keep playing while you browse",
            "Music or a film you play goes on when you select another file, in a player at the foot of the preview. Off, it stops when the selection moves.",
            true,
        )
        .section("Music and films"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            AUTOPLAY_KEY,
            "Play on selection",
            "Start music and films as soon as they are selected, instead of waiting for Play.",
            false,
        )
        .section("Music and films"),
    );
    app.settings.declare(
        SettingSpec::choice(
            CONTINUE_KEY,
            "When a track ends",
            "Stop, go on with the next file of the same kind in the folder (or the playlist), or with one of them at random.",
            crate::player::Continue::Stop.key(),
            &crate::player::Continue::ALL.map(|mode| (mode.key(), mode.label())),
        )
        .section("Music and films"),
    );
    app.settings.declare(SettingSpec::data(
        crate::shortcuts::HINTS_KEY,
        "Commands whose shortcut hint was already shown",
        toml::Value::Array(Vec::new()),
    ));
    app.settings.declare(SettingSpec::data(
        COLUMN_WIDTHS_KEY,
        "Widths of the Size, Modified and Type columns, in pixels",
        toml::Value::Array(
            crate::views::list::DEFAULT_WIDTHS
                .iter()
                .map(|width| toml::Value::Integer(*width as i64))
                .collect(),
        ),
    ));
    for (key, title, description) in [
        (COLUMN_KEYS[0], "Size", "Show the size of files."),
        (
            COLUMN_KEYS[1],
            "Modified",
            "Show when each item last changed.",
        ),
        (
            COLUMN_KEYS[2],
            "Type",
            "Show the kind of file, such as PDF document. It is the first column to give way in a narrow pane.",
        ),
    ] {
        app.settings
            .declare(SettingSpec::toggle(key, title, description, true).section("List columns"));
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
    Plugins,
    Keyboard,
    Plugin(String),
}

impl Page {
    pub fn of_owner(owner: &str) -> Page {
        match owner {
            CORE_OWNER => Page::General,
            UI_OWNER => Page::Appearance,
            other => Page::Plugin(other.to_owned()),
        }
    }

    fn parse(name: &str) -> Page {
        match name {
            "general" => Page::General,
            "appearance" => Page::Appearance,
            "plugins" => Page::Plugins,
            "keyboard" => Page::Keyboard,
            other => Page::Plugin(other.to_owned()),
        }
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

    pub fn modified(&self) -> bool {
        self.value != self.default_text()
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
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

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
            app,
            font,
            key_hints,
            page: Page::General,
            search,
            rows: Vec::new(),
            requirements: None,
            probe: Arc::new(Mutex::new(None)),
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

    pub(crate) fn page_title(&self, page: &Page) -> String {
        match page {
            Page::General => "General".into(),
            Page::Appearance => "Appearance".into(),
            Page::Plugins => "Plugins".into(),
            Page::Keyboard => "Keyboard".into(),
            Page::Plugin(id) => self.plugin_title(id),
        }
    }

    /// Sidebar entries: the fixed pages, then every plugin that has settings.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let mut pages = vec![
            Page::General,
            Page::Appearance,
            Page::Plugins,
            Page::Keyboard,
        ];
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
                .notify(bibli_core::app::NotifyLevel::Error, format!("{error:#}")),
        }
        cx.notify();
    }

    pub(crate) fn probe_requirements(&mut self) {
        let slot = Arc::clone(&self.probe);
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Normal,
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
            bibli_core::scheduler::Priority::Normal,
            "settings-open-file",
            self.app.lifetime().child(),
            move || {
                use bibli_core::app::NotifyLevel;
                let Some(app) = weak.upgrade() else { return };
                if !path.is_file() {
                    app.notify(
                        NotifyLevel::Info,
                        "There is no settings file yet: every setting has its default value",
                    );
                } else if let Err(error) = bibli_platform::open::open_path(&path) {
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
            self.key_hints.clone(),
        );
        let settings = cx.new(|cx| SettingsView::new(app, font, hints, target, window, cx));
        self.settings_subscription = Some(cx.subscribe_in(
            &settings,
            window,
            |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Close => {
                    this.settings = None;
                    this.settings_subscription = None;
                    window.focus(&this.ui().focus);
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
