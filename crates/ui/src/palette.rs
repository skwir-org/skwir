//! Command palette: fuzzy search over every command, run on Enter, prompt for
//! required string arguments in sequence.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::theme::Chosen;
use std::sync::OnceLock;

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*};
use skwir_core::SortKey;
use skwir_core::words;
use skwir_core::words::Words;

use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn, Picker};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub name: String,
    pub args: serde_json::Value,
    pub title: String,
    pub keys: Option<String>,
    /// Required argument names from the command's schema.
    pub required: Vec<String>,
}

pub struct ArgPrompt {
    pub name: String,
    pub required: Vec<String>,
    pub values: serde_json::Map<String, serde_json::Value>,
    pub index: usize,
}

pub struct PaletteState {
    pub input: Entity<TextInput>,
    pub items: Vec<PaletteItem>,
    pub filtered: Vec<usize>,
    pub picker: Picker,
    query: Option<String>,
    pub args: Option<ArgPrompt>,
    back: FocusReturn,
    _sub: Subscription,
}

/// The shell's own commands the palette offers, each with the arguments
/// that make it one thing to do, alongside the core's and the plugins'.
/// Every one is declared (`commands.rs`); a test checks its arguments.
fn ui_command_table() -> Vec<(&'static str, &'static str, Words)> {
    vec![
        (
            "ui.focus_address",
            "{}",
            words!(palette_focus_address = "Focus address bar"),
        ),
        (
            "ui.focus_pane",
            "{}",
            words!(palette_focus_pane = "Focus file list"),
        ),
        (
            "ui.palette",
            "{}",
            words!(palette_command_palette = "Command palette"),
        ),
        ("ui.settings", "{}", words!(palette_settings = "Settings…")),
        ("ui.about", "{}", words!(palette_about = "About Skwir")),
        (
            "ui.rename_prompt",
            "{}",
            words!(palette_rename_prompt = "Rename…"),
        ),
        (
            "ui.mkdir_prompt",
            "{}",
            words!(palette_mkdir_prompt = "New folder…"),
        ),
        (
            "ui.new_file_prompt",
            "{}",
            words!(palette_new_file_prompt = "New file…"),
        ),
        ("ui.find", "{}", words!(palette_find = "Find a name…")),
        (
            "ui.find",
            r#"{"backward":true}"#,
            words!(palette_find_backward = "Find a name before the cursor…"),
        ),
        (
            "ui.find_next",
            r#"{"delta":1}"#,
            words!(palette_find_next = "Find the next name"),
        ),
        (
            "ui.find_next",
            r#"{"delta":-1}"#,
            words!(palette_find_previous = "Find the previous name"),
        ),
        (
            "ui.toggle_sidebar",
            "{}",
            words!(palette_toggle_sidebar = "Toggle sidebar"),
        ),
        (
            "ui.focus_sidebar",
            "{}",
            words!(palette_focus_sidebar = "Focus sidebar"),
        ),
        (
            "ui.focus_next_part",
            "{}",
            words!(palette_focus_next_part = "Focus next part of the window"),
        ),
        (
            "ui.sidebar_section",
            r#"{"reset":true}"#,
            words!(palette_sidebar_section_reset = "Sidebar: show every group in its place"),
        ),
        (
            "ui.grid_fill",
            "{}",
            words!(palette_grid_fill = "Toggle filled thumbnails"),
        ),
        (
            "ui.view_buttons_shown",
            "{}",
            words!(palette_view_buttons_shown = "Toggle view buttons"),
        ),
        (
            "ui.up_button",
            "{}",
            words!(palette_up_button = "Toggle the Up button"),
        ),
        (
            "ui.quick_filters",
            "{}",
            words!(palette_quick_filters = "Toggle Quick filters"),
        ),
        (
            "ui.full_width",
            "{}",
            words!(palette_full_width = "Toggle full-width reading"),
        ),
        (
            "ui.usage_mode",
            r#"{"mode":"size"}"#,
            words!(palette_usage_mode_size = "Usage: blocks by size"),
        ),
        (
            "ui.usage_mode",
            r#"{"mode":"files"}"#,
            words!(palette_usage_mode_files = "Usage: blocks by files"),
        ),
        (
            "ui.usage_mode",
            r#"{"mode":"age"}"#,
            words!(palette_usage_mode_age = "Usage: color by age"),
        ),
        (
            "ui.usage_mode",
            r#"{"mode":"types"}"#,
            words!(palette_usage_mode_types = "Usage: color by kind"),
        ),
        (
            "ui.zoom",
            r#"{"dir":"in"}"#,
            words!(palette_zoom_in = "Zoom in"),
        ),
        (
            "ui.zoom",
            r#"{"dir":"out"}"#,
            words!(palette_zoom_out = "Zoom out"),
        ),
        (
            "ui.zoom",
            r#"{"dir":"reset"}"#,
            words!(palette_zoom_reset = "Zoom reset"),
        ),
        (
            "ui.text_size",
            r#"{"part":"files","dir":"in"}"#,
            words!(palette_text_size_files_in = "Files text: larger"),
        ),
        (
            "ui.text_size",
            r#"{"part":"files","dir":"out"}"#,
            words!(palette_text_size_files_out = "Files text: smaller"),
        ),
        (
            "ui.text_size",
            r#"{"part":"files","dir":"reset"}"#,
            words!(palette_text_size_files_reset = "Files text: 100%"),
        ),
        (
            "ui.map_view_fit",
            "{}",
            words!(palette_map_view_fit = "Map: fit to its files"),
        ),
        (
            "ui.map_view_search_area",
            "{}",
            words!(
                palette_map_view_search_area = "Map: search the pictures and videos of this area"
            ),
        ),
        (
            "ui.map_view_spread",
            "{}",
            words!(palette_map_view_spread = "Map: spread the files at one place, or group them"),
        ),
        (
            "ui.toggle_preview",
            "{}",
            words!(palette_toggle_preview = "Toggle preview"),
        ),
        (
            "ui.terminal",
            "{}",
            words!(palette_terminal = "Toggle terminal"),
        ),
        (
            "ui.terminal_copy",
            "{}",
            words!(palette_terminal_copy = "Terminal: copy"),
        ),
        (
            "ui.terminal_paste",
            "{}",
            words!(palette_terminal_paste = "Terminal: paste"),
        ),
        (
            "ui.terminal_select_all",
            "{}",
            words!(palette_terminal_select_all = "Terminal: select all"),
        ),
        (
            "ui.terminal_clear",
            "{}",
            words!(palette_terminal_clear = "Terminal: clear the history"),
        ),
        (
            "ui.terminal_paths",
            "{}",
            words!(palette_terminal_paths = "Terminal: type the selection's paths"),
        ),
        (
            "ui.terminal_links",
            "{}",
            words!(palette_terminal_links = "Terminal: follow a link…"),
        ),
        (
            "ui.inspect",
            r#"{"action":"links"}"#,
            words!(palette_inspect_links = "Preview: follow a link…"),
        ),
        (
            "ui.preview_page",
            r#"{"delta":-1}"#,
            words!(palette_preview_page_previous = "Preview: previous page"),
        ),
        (
            "ui.preview_page",
            r#"{"delta":1}"#,
            words!(palette_preview_page_next = "Preview: next page"),
        ),
        (
            "ui.preview_goto",
            "{}",
            words!(palette_preview_goto = "Preview: go to page or chapter…"),
        ),
        (
            "ui.preview_open_external",
            "{}",
            words!(palette_preview_open_external = "Preview: open externally"),
        ),
        (
            "ui.preview_copy_path",
            "{}",
            words!(palette_preview_copy_path = "Preview: copy path"),
        ),
        (
            "ui.media_play",
            "{}",
            words!(palette_media_play = "Player: play the selection"),
        ),
        (
            "ui.media_pin",
            "{}",
            words!(palette_media_pin = "Player: pin or let go of this track"),
        ),
        (
            "ui.media_keep_playing",
            "{}",
            words!(palette_media_keep_playing = "Player: keep playing while you browse"),
        ),
        (
            "ui.media_autoplay",
            "{}",
            words!(palette_media_autoplay = "Player: play on selection"),
        ),
        (
            "ui.media_continue",
            r#"{"mode":"off"}"#,
            words!(palette_media_continue_off = "Player: stop when a track ends"),
        ),
        (
            "ui.media_continue",
            r#"{"mode":"next"}"#,
            words!(palette_media_continue_next = "Player: play the next track when one ends"),
        ),
        (
            "ui.media_continue",
            r#"{"mode":"shuffle"}"#,
            words!(palette_media_continue_shuffle = "Player: shuffle"),
        ),
        (
            "ui.player",
            r#"{"request":"play_pause"}"#,
            words!(palette_player_play_pause = "Player: play or pause"),
        ),
        (
            "ui.player",
            r#"{"request":"stop"}"#,
            words!(palette_player_stop = "Player: stop and let go"),
        ),
        (
            "ui.media_step",
            r#"{"delta":1}"#,
            words!(palette_media_next = "Player: next track"),
        ),
        (
            "ui.media_step",
            r#"{"delta":-1}"#,
            words!(palette_media_previous = "Player: previous track"),
        ),
        (
            "ui.properties",
            "{}",
            words!(palette_properties = "Toggle properties"),
        ),
        (
            "ui.inspect",
            r#"{"action":"hex"}"#,
            words!(palette_inspect_hex = "Preview: hexadecimal"),
        ),
        (
            "ui.inspect",
            r#"{"action":"auto"}"#,
            words!(palette_inspect_auto = "Preview: default presentation"),
        ),
        (
            "ui.preview_find",
            "{}",
            words!(palette_preview_find = "Preview: find text"),
        ),
        (
            "ui.preview_match",
            r#"{"delta":1}"#,
            words!(palette_preview_match_next = "Preview: next search match"),
        ),
        (
            "ui.preview_match",
            r#"{"delta":-1}"#,
            words!(palette_preview_match_previous = "Preview: previous search match"),
        ),
        (
            "ui.inspect",
            r#"{"action":"source"}"#,
            words!(palette_inspect_source = "Preview: source / rendered"),
        ),
        (
            "ui.inspect",
            r#"{"action":"turn"}"#,
            words!(palette_inspect_turn = "Preview: 3D / picture"),
        ),
        (
            "ui.inspect",
            r#"{"action":"whole"}"#,
            words!(palette_inspect_whole = "Preview: full preview"),
        ),
        (
            "ui.model_perspective",
            "{}",
            words!(palette_model_perspective = "Model: perspective"),
        ),
        (
            "ui.model_smooth",
            "{}",
            words!(palette_model_smooth = "Model: smooth shading"),
        ),
        (
            "ui.wrap_lines",
            "{}",
            words!(palette_wrap_lines = "Preview: wrap lines"),
        ),
        (
            "ui.text_continuous",
            "{}",
            words!(palette_text_continuous = "Preview: toggle continuous scrolling of text"),
        ),
        (
            "ui.preview_contents",
            "{}",
            words!(palette_preview_contents = "Preview: contents"),
        ),
        (
            "ui.contents_pin",
            "{}",
            words!(palette_contents_pin = "Preview: keep contents open"),
        ),
        (
            "ui.preview_copy",
            "{}",
            words!(palette_preview_copy = "Preview: copy"),
        ),
        (
            "ui.preview_select_all",
            "{}",
            words!(palette_preview_select_all = "Preview: select all"),
        ),
        (
            "ui.preview_close",
            "{}",
            words!(palette_preview_close = "Preview: close"),
        ),
        (
            "ui.preview_zoom",
            r#"{"dir":"width"}"#,
            words!(palette_preview_zoom_width = "Preview: fit width"),
        ),
        (
            "ui.preview_zoom",
            r#"{"dir":"1"}"#,
            words!(palette_preview_zoom_actual = "Preview: actual size (100%)"),
        ),
        (
            "ui.reader_continuous",
            "{}",
            words!(palette_reader_continuous = "Document: toggle continuous scrolling"),
        ),
        (
            "ui.reader_drag",
            "{}",
            words!(palette_reader_drag = "Document: toggle dragging to move the page"),
        ),
        (
            "ui.preview_seek",
            r#"{"delta":-5}"#,
            words!(palette_preview_seek_back = "Preview: back five seconds"),
        ),
        (
            "ui.preview_seek",
            r#"{"delta":5}"#,
            words!(palette_preview_seek_forward = "Preview: forward five seconds"),
        ),
        (
            "ui.preview_zoom",
            r#"{"dir":"in"}"#,
            words!(palette_preview_zoom_in = "Preview: zoom in"),
        ),
        (
            "ui.preview_zoom",
            r#"{"dir":"out"}"#,
            words!(palette_preview_zoom_out = "Preview: zoom out"),
        ),
        (
            "ui.preview_zoom",
            r#"{"dir":"reset"}"#,
            words!(palette_preview_zoom_reset = "Preview: fit"),
        ),
        (
            "ui.preview_fullscreen",
            "{}",
            words!(palette_preview_fullscreen = "Preview: full screen"),
        ),
        (
            "ui.slideshow",
            "{}",
            words!(palette_slideshow = "Preview: slideshow"),
        ),
        (
            "ui.map_open",
            "{}",
            words!(palette_map_open = "Map: open in Maps"),
        ),
        ("ui.peek", "{}", words!(palette_peek = "Peek")),
        ("ui.search", "{}", words!(palette_search = "Search…")),
        ("ui.filter", "{}", words!(palette_filter = "Filter…")),
        (
            "ui.history_menu",
            "{}",
            words!(palette_history_menu = "Back: history…"),
        ),
        (
            "ui.history_menu",
            r#"{"forward":true}"#,
            words!(palette_history_menu_forward = "Forward: history…"),
        ),
        ("ui.jump", "{}", words!(palette_jump = "Jump…")),
        (
            "ui.rename_bulk_prompt",
            "{}",
            words!(palette_rename_bulk_prompt = "Rename several…"),
        ),
        (
            "ui.tags_prompt",
            "{}",
            words!(palette_tags_prompt = "Tags…"),
        ),
        ("ui.timeline", "{}", words!(palette_timeline = "Timeline")),
        ("ui.split", "{}", words!(palette_split = "Split pane")),
        (
            "ui.split",
            r#"{"axis":"other"}"#,
            words!(palette_split_other = "Split pane the other way"),
        ),
        (
            "ui.layout",
            r#"{"panes":"auto"}"#,
            words!(palette_layout_panes_auto = "Panes: arrange automatically"),
        ),
        (
            "ui.layout",
            r#"{"panes":"row"}"#,
            words!(palette_layout_panes_row = "Panes: side by side"),
        ),
        (
            "ui.layout",
            r#"{"panes":"column"}"#,
            words!(palette_layout_panes_column = "Panes: one above the other"),
        ),
        (
            "ui.layout",
            r#"{"preview":"auto"}"#,
            words!(palette_layout_preview_auto = "Preview: place automatically"),
        ),
        (
            "ui.layout",
            r#"{"preview":"right"}"#,
            words!(palette_layout_preview_right = "Preview: on the right"),
        ),
        (
            "ui.layout",
            r#"{"preview":"bottom"}"#,
            words!(palette_layout_preview_bottom = "Preview: at the bottom"),
        ),
        (
            "ui.layout",
            r#"{"member":"auto"}"#,
            words!(palette_layout_member_auto = "File in an archive: place automatically"),
        ),
        (
            "ui.layout",
            r#"{"member":"right"}"#,
            words!(palette_layout_member_right = "File in an archive: on the right"),
        ),
        (
            "ui.layout",
            r#"{"member":"bottom"}"#,
            words!(palette_layout_member_bottom = "File in an archive: at the bottom"),
        ),
        (
            "ui.focus_other_pane",
            "{}",
            words!(palette_focus_other_pane = "Focus other pane"),
        ),
        ("ui.quit", "{}", words!(palette_quit = "Quit")),
    ]
}

/// The shell's own commands the palette offers ([`ui_command_table`]) with
/// their arguments parsed, once: each called by its words, said where it is
/// shown.
pub fn ui_commands() -> &'static [(&'static str, serde_json::Value, Words)] {
    static PARSED: OnceLock<Vec<(&'static str, serde_json::Value, Words)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        ui_command_table()
            .into_iter()
            .map(|(name, args, title)| {
                (
                    name,
                    serde_json::from_str(args).expect("valid ui args"),
                    title,
                )
            })
            .collect()
    })
}

/// One thing the palette offers to do: a command with the arguments that
/// make it that thing, what it is called, and what it still asks for.
#[derive(Clone, Debug)]
pub struct Offer {
    pub name: String,
    pub args: serde_json::Value,
    /// What it is called, as words other rows carry (the address field's
    /// actions).
    pub words: Words,
    /// The same, said as the palette was asked: what typing matches.
    pub title: String,
    /// Required argument names from the command's schema.
    pub required: Vec<String>,
}

impl Offer {
    fn of(name: &str, args: serde_json::Value, title: impl Into<Words>) -> Self {
        let words = title.into();
        Offer {
            name: name.to_owned(),
            args,
            title: words.say(),
            words,
            required: Vec::new(),
        }
    }
}

/// Everything the palette offers to do, with `views` (those the window
/// shows) and `sorts` (what the pane sorts by there, `Shell::sort_keys`):
/// each core and plugin command as it is and each thing its forms make of
/// it (Sort by size, Next tab), the shell's own as `UI_COMMANDS` says, a
/// view, a setting by name. Each is called by a title of its own
/// (`crates/skwir/tests/keymap_titles.rs`).
pub fn offered(
    app: &skwir_core::App,
    views: &[skwir_core::registry::ViewSpec],
    sorts: &[SortKey],
    groups: &[SortKey],
) -> Vec<Offer> {
    let mut offers: Vec<Offer> = Vec::new();
    for spec in app.commands.list() {
        // Structured preference batches are edited by the Settings dialog;
        // the shell's own are offered as `UI_COMMANDS` says, most being
        // keys of a place (a menu's arrows), nothing to run from here; and
        // what is data (an agent's question, a panel's rows) shows nothing
        // run from here.
        if matches!(&*spec.name, "settings.apply" | "plugins.set_enabled")
            || &*spec.owner == crate::commands::OWNER
            || spec.data
        {
            continue;
        }
        offers.push(Offer {
            required: spec.required_args(),
            ..Offer::of(&spec.name, serde_json::json!({}), spec.title.clone())
        });
        for form in &spec.forms {
            // A sort by what no column shows here is not offered, as the
            // View menu does not offer it (by date deleted, outside the
            // trash).
            let key = form
                .args
                .get("key")
                .and_then(|key| serde_json::from_value::<SortKey>(key.clone()).ok());
            if &*spec.name == "sort" && key.is_some_and(|key| !sorts.contains(&key)) {
                continue;
            }
            // Grouped likewise, and not at all without the sorter, which
            // makes every group (ADR-130).
            if &*spec.name == "group" {
                let by = form.args.get("by").cloned().and_then(|by| {
                    serde_json::from_value::<skwir_core::grouping::GroupBy>(by).ok()
                });
                let offered = match by {
                    Some(skwir_core::grouping::GroupBy::Key(key)) => groups.contains(&key),
                    _ => !groups.is_empty(),
                };
                if !offered {
                    continue;
                }
            }
            offers.push(Offer::of(&spec.name, form.args.clone(), form.title.clone()));
        }
    }
    for view in views {
        offers.push(Offer::of(
            "ui.view",
            serde_json::json!({ "id": view.id }),
            keymap::view_title(view.title.clone()),
        ));
    }
    // Every setting is reachable by name, like a command: its page, its
    // section (a store of notes reads and writes), its title.
    let plugins = app.plugins.list();
    for spec in app.settings.schema() {
        if !spec.editable() {
            continue;
        }
        let page = crate::settings::page_title_of(&spec, &plugins);
        let title = if spec.section.is_empty() {
            words!(
                palette_setting = "Settings: { $page } › { $setting }",
                page = page,
                setting = spec.title.clone(),
                note = "A command of the palette opening a setting; page: the settings page it is on; setting: its title."
            )
        } else {
            words!(
                palette_setting_in_section = "Settings: { $page } › { $section } › { $setting }",
                page = page,
                section = spec.section_title.clone(),
                setting = spec.title.clone(),
                note = "A command of the palette opening a setting; page: the settings page it is on; section: the section of the page; setting: its title."
            )
        };
        offers.push(Offer::of(
            "ui.settings",
            serde_json::json!({ "key": spec.key }),
            title,
        ));
    }
    for (page, title) in [
        (
            "plugins",
            words!(
                palette_settings_plugins = "Settings: Plugins",
                note = "A command of the palette opening the settings' page of plugins."
            ),
        ),
        (
            "keyboard",
            words!(
                palette_settings_keyboard = "Settings: Keyboard shortcuts",
                note = "A command of the palette opening the settings' page of keyboard shortcuts."
            ),
        ),
    ] {
        offers.push(Offer::of(
            "ui.settings",
            serde_json::json!({ "page": page }),
            title,
        ));
    }
    for (name, args, title) in ui_commands() {
        offers.push(Offer::of(name, args.clone(), title.clone()));
    }
    // A plugin's context-menu item that runs a shell command with its
    // own arguments (Other Application… is `ui.choose` over the plugin's
    // list) has no command of its own to be listed by: list the item, by
    // its submenu's name and its own where it is in one.
    for item in app.registry.menus.all() {
        let command = &item.command;
        if item.menu != "context"
            || !command.name.starts_with("ui.")
            || offers
                .iter()
                .any(|o| *o.name == *command.name && o.args == command.args)
        {
            continue;
        }
        let title = match &item.submenu {
            Some(submenu) => words!(
                palette_in_submenu = "{ $submenu } › { $item }",
                submenu = submenu.title.clone(),
                item = item.title.clone(),
                note = "A context menu's item in the palette, named by its submenu (Open With) and its own title."
            ),
            None => item.title.clone(),
        };
        offers.push(Offer::of(&command.name, command.args.clone(), title));
    }
    offers
}

impl Shell {
    fn palette_items(&self) -> Vec<PaletteItem> {
        let sorts: Vec<SortKey> = self.sort_keys();
        offered(
            &self.app,
            &self.available_views(),
            &sorts,
            &self.group_keys(),
        )
        .into_iter()
        .map(|offer| PaletteItem {
            keys: self.keys_for(&offer.name, &offer.args),
            name: offer.name,
            args: offer.args,
            title: offer.title,
            required: offer.required,
        })
        .collect()
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| {
            let mut input = TextInput::new(words!(palette_placeholder = "Type a command"), cx);
            input.set_label(palette_label());
            input
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.palette_submit(window, cx),
                InputEvent::Cancel => this.close_palette(window, cx),
                InputEvent::Changed => cx.notify(),
            },
        );
        input.update(cx, |i, cx| i.focus(window, cx));
        let items = self.palette_items();
        self.palette = Some(PaletteState {
            input,
            filtered: (0..items.len()).collect(),
            items,
            picker: Picker::default(),
            query: None,
            args: None,
            back,
            _sub: sub,
        });
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            palette.back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
    }

    pub fn palette_move(&mut self, mv: modal::ListMove, cx: &mut Context<Self>) {
        // What was typed since the last frame filters first, as `choose`'s
        // does: a key right after a character acts on the list it made.
        self.palette_refilter(cx);
        if let Some(p) = &mut self.palette {
            p.picker.go(mv, p.filtered.len());
            cx.notify();
        }
    }

    fn palette_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_refilter(cx);
        let Some(p) = &mut self.palette else { return };
        if let Some(args) = &mut p.args {
            // Collecting required arguments one at a time.
            let value = p.input.read(cx).text().to_owned();
            let key = args.required[args.index].clone();
            args.values.insert(key, serde_json::Value::String(value));
            args.index += 1;
            if args.index < args.required.len() {
                let asked = asked(&args.name, &args.required[args.index]);
                p.input.update(cx, |i, cx| {
                    i.set_text("", cx);
                    i.set_label(asked);
                });
                cx.notify();
                return;
            }
            let name = args.name.clone();
            let values = serde_json::Value::Object(args.values.clone());
            self.close_palette(window, cx);
            self.run(&name, values, window, cx);
            return;
        }
        let Some(&ix) = p.filtered.get(p.picker.highlighted) else {
            return;
        };
        let item = p.items[ix].clone();
        if item.required.is_empty() {
            self.close_palette(window, cx);
            self.run(&item.name, item.args, window, cx);
        } else {
            // The field is named by what it asks for, as its label says.
            let asked = asked(&item.name, &item.required[0]);
            p.args = Some(ArgPrompt {
                name: item.name,
                required: item.required,
                values: serde_json::Map::new(),
                index: 0,
            });
            p.input.update(cx, |i, cx| {
                i.set_text("", cx);
                i.set_label(asked);
            });
            cx.notify();
        }
    }

    fn palette_refilter(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else { return };
        if p.args.is_some() {
            return;
        }
        let q = p.input.read(cx).text().to_owned();
        if p.query.as_deref() == Some(q.as_str()) {
            return;
        }
        p.filtered = modal::fuzzy(&p.items, &q, |it| format!("{} {}", it.title, it.name));
        p.picker = Picker::default();
        p.query = Some(q);
    }
}

/// What the palette's field is named while it asks for no argument.
fn palette_label() -> Words {
    words!(
        palette_label = "Command",
        note = "The label before the palette's field, where a command's name is typed."
    )
}

/// What the palette's field is named while it asks for `argument` of
/// `command`: both identifiers, as written.
fn asked(command: &str, argument: &str) -> Words {
    words!(
        palette_asks = "{ $command } · { $argument }",
        command = command,
        argument = argument,
        note = "The label before the palette's field while it asks for an argument; command: the command's name; argument: the argument's name (both identifiers)."
    )
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.palette_refilter(cx);
    let p = shell.palette.as_ref()?;
    let theme = cx.global::<Theme>().clone();

    let label = match &p.args {
        Some(a) => asked(&a.name, &a.required[a.index]).say(),
        None => palette_label().say(),
    };
    let mut panel = modal::panel("palette", 560., &theme)
        .aria_label(
            words!(
                palette_name = "Command palette",
                note = "What a screen reader calls the palette's panel."
            )
            .say(),
        )
        .max_h(sized(420.))
        .key_context(keymap::PALETTE)
        .child(
            div()
                .p_2()
                .flex()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_body().text_color(theme.text_muted).child(label))
                .child(div().flex_1().child(p.input.clone())),
        );
    if p.args.is_none() {
        // The commands found, a list box beside the field that keeps the
        // keyboard: the highlighted one is said as focused while the field
        // has the focus.
        let a11y = shell.a11y;
        let typing = gpui::Focusable::focus_handle(p.input.read(cx), cx);
        let mut list = modal::options(
            "palette-results",
            words!(
                palette_commands = "Commands",
                note = "What a screen reader calls the palette's list of commands found."
            )
            .say(),
            p.filtered.len(),
            a11y,
        )
        .flex()
        .flex_col()
        .py_1()
        .overflow_hidden();
        for row_ix in p.picker.shown(p.filtered.len()) {
            let item = &p.items[p.filtered[row_ix]];
            let hl = row_ix == p.picker.highlighted;
            // Drawn in the selection's ink where chosen (`Chosen`).
            let muted = theme.text_muted;
            let name = item.name.clone();
            let option = modal::option(
                div().id(crate::elements::known_by("cmd", p.filtered[row_ix])),
                a11y.then(|| item.title.clone().into()),
                row_ix,
                hl,
                &typing,
            )
            .when(a11y, |d| {
                d.aria_description(name.clone()).when_some(
                    item.keys.clone(),
                    StatefulInteractiveElement::aria_keyshortcuts,
                )
            });
            list = list.child(
                option
                    .h(sized(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .when(hl, |d| d.chosen(&theme))
                    .when(!hl, |d| d.hover(|s| s.bg(theme.hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(p) = &mut this.palette {
                            p.picker.highlighted = row_ix;
                        }
                        this.palette_submit(window, cx);
                    }))
                    // One line, a title too long cut, said whole when
                    // pointed at; the command's name and keys kept whole.
                    .child(crate::elements::clip::clipped(
                        div().id("command-title").min_w_0().truncate(),
                        item.title.clone(),
                    ))
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_small()
                            .text_color(muted)
                            .child(name),
                    )
                    .child(div().flex_1())
                    .children(item.keys.clone().map(|k| {
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_small()
                            .text_color(muted)
                            .child(crate::sides::shortcut(k))
                    })),
            );
        }
        if p.filtered.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(theme.text_muted)
                    .child(words!(palette_none = "No matching command").say()),
            );
        }
        let list = modal::scrolled(
            list,
            "palette-results",
            &p.picker,
            p.filtered.len(),
            modal::PICKER_ROWS,
            26.,
            |shell| shell.palette.as_mut().map(|p| &mut p.picker),
            cx,
        );
        panel = panel.child(list);
    }
    Some(modal::modal(
        panel,
        80.,
        4,
        Shell::close_palette,
        &theme,
        cx,
    ))
}

#[cfg(test)]
mod tests {
    use skwir_core::SortKey;
    use skwir_core::app::{App, AppConfig};
    use skwir_core::command::{NoArgs, Outcome};
    use skwir_core::words::Words;

    fn titles(app: &App, sorts: &[SortKey]) -> Vec<String> {
        super::offered(app, &[], sorts, &[])
            .into_iter()
            .map(|offer| offer.title)
            .collect()
    }

    /// What only answers is left out; what changes nothing yet shows
    /// something run from here (a text copied) is offered.
    #[test]
    fn data_is_not_offered_and_what_shows_is() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let copied = || {
            Ok(Outcome::Clipboard {
                text: "/a".into(),
                said: Words::as_written("Copied"),
            })
        };
        app.commands
            .register::<NoArgs, _>(
                "test",
                skwir_core::command::Topic::Files,
                "test.copy",
                Words::as_written("Copy a test text"),
                Words::as_written(""),
                move |_, _| copied(),
            )
            .reads();
        app.commands
            .register::<NoArgs, _>(
                "test",
                skwir_core::command::Topic::Files,
                "test.rows",
                Words::as_written("Rows of a test panel"),
                Words::as_written(""),
                |_, _| Ok(Outcome::Value(serde_json::json!([]))),
            )
            .data();
        let offered = titles(&app, &[SortKey::Name]);
        assert!(offered.iter().any(|t| t == "Copy a test text"));
        assert!(!offered.iter().any(|t| t == "Rows of a test panel"));
        // The core's own data: the tabs, the entries, the commands.
        for data in ["List tabs", "List entries"] {
            assert!(!offered.iter().any(|t| t == data), "{data}");
        }
    }

    /// The sorts offered are the View menu's: by what a column shows here,
    /// in both directions, and either direction of the sort in force.
    #[test]
    fn a_sort_by_what_no_column_shows_is_not_offered() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let folder = titles(&app, &[SortKey::Name, SortKey::Size, SortKey::Modified]);
        for offered in [
            "Sort ascending",
            "Sort descending",
            "Sort by name",
            "Sort by size, largest first",
            "Sort by date modified",
        ] {
            assert!(folder.iter().any(|t| t == offered), "{offered}");
        }
        assert!(!folder.iter().any(|t| t.starts_with("Sort by date deleted")));
        assert!(!folder.iter().any(|t| t.starts_with("Sort by rating")));
        let trash = titles(&app, &[SortKey::Name, SortKey::Deleted]);
        assert!(
            trash
                .iter()
                .any(|t| t == "Sort by date deleted, newest first")
        );
        assert!(!trash.iter().any(|t| t.starts_with("Sort by size")));
    }
}
