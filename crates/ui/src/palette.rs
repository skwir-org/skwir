//! Command palette: fuzzy search over every command, run on Enter, prompt for
//! required string arguments in sequence.

use std::sync::OnceLock;

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use skwir_core::SortKey;

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
pub const UI_COMMANDS: &[(&str, &str, &str)] = &[
    ("ui.focus_address", "{}", "Focus address bar"),
    ("ui.focus_pane", "{}", "Focus file list"),
    ("ui.palette", "{}", "Command palette"),
    ("ui.settings", "{}", "Settings…"),
    ("ui.about", "{}", "About skwir"),
    ("ui.rename_prompt", "{}", "Rename…"),
    ("ui.mkdir_prompt", "{}", "New folder…"),
    ("ui.new_file_prompt", "{}", "New file…"),
    ("ui.find", "{}", "Find a name…"),
    (
        "ui.find",
        r#"{"backward":true}"#,
        "Find a name before the cursor…",
    ),
    ("ui.find_next", r#"{"delta":1}"#, "Find the next name"),
    ("ui.find_next", r#"{"delta":-1}"#, "Find the previous name"),
    ("ui.toggle_sidebar", "{}", "Toggle sidebar"),
    ("ui.focus_sidebar", "{}", "Focus sidebar"),
    ("ui.focus_next_part", "{}", "Focus next part of the window"),
    (
        "ui.sidebar_section",
        r#"{"reset":true}"#,
        "Sidebar: show every group in its place",
    ),
    ("ui.grid_fill", "{}", "Toggle filled thumbnails"),
    (
        "ui.narrow_folders",
        "{}",
        "Toggle the Narrow by bar over folders",
    ),
    ("ui.full_width", "{}", "Toggle full-width reading"),
    (
        "ui.usage_mode",
        r#"{"mode":"size"}"#,
        "Usage: blocks by size",
    ),
    (
        "ui.usage_mode",
        r#"{"mode":"files"}"#,
        "Usage: blocks by files",
    ),
    ("ui.usage_mode", r#"{"mode":"age"}"#, "Usage: color by age"),
    (
        "ui.usage_mode",
        r#"{"mode":"types"}"#,
        "Usage: color by kind",
    ),
    ("ui.zoom", r#"{"dir":"in"}"#, "Zoom in"),
    ("ui.zoom", r#"{"dir":"out"}"#, "Zoom out"),
    ("ui.zoom", r#"{"dir":"reset"}"#, "Zoom reset"),
    ("ui.map_view_fit", "{}", "Map: fit to its files"),
    (
        "ui.map_view_search_area",
        "{}",
        "Map: search the pictures and videos of this area",
    ),
    (
        "ui.map_view_spread",
        "{}",
        "Map: spread the files at one place, or group them",
    ),
    ("ui.toggle_preview", "{}", "Toggle preview panel"),
    ("ui.terminal", "{}", "Toggle terminal"),
    ("ui.terminal_copy", "{}", "Terminal: copy"),
    ("ui.terminal_paste", "{}", "Terminal: paste"),
    ("ui.terminal_select_all", "{}", "Terminal: select all"),
    ("ui.terminal_clear", "{}", "Terminal: clear the history"),
    (
        "ui.terminal_paths",
        "{}",
        "Terminal: type the selection's paths",
    ),
    ("ui.terminal_links", "{}", "Terminal: follow a link…"),
    (
        "ui.inspect",
        r#"{"action":"links"}"#,
        "Preview: follow a link…",
    ),
    (
        "ui.preview_page",
        r#"{"delta":-1}"#,
        "Preview: previous page",
    ),
    ("ui.preview_page", r#"{"delta":1}"#, "Preview: next page"),
    ("ui.preview_goto", "{}", "Preview: go to page or chapter…"),
    ("ui.preview_open_external", "{}", "Preview: open externally"),
    ("ui.preview_copy_path", "{}", "Preview: copy path"),
    ("ui.media_pin", "{}", "Player: pin or let go of this track"),
    (
        "ui.media_keep_playing",
        "{}",
        "Player: keep playing while you browse",
    ),
    ("ui.media_autoplay", "{}", "Player: play on selection"),
    (
        "ui.media_continue",
        r#"{"mode":"off"}"#,
        "Player: stop when a track ends",
    ),
    (
        "ui.media_continue",
        r#"{"mode":"next"}"#,
        "Player: play the next track when one ends",
    ),
    (
        "ui.media_continue",
        r#"{"mode":"shuffle"}"#,
        "Player: shuffle",
    ),
    (
        "ui.player",
        r#"{"request":"play_pause"}"#,
        "Player: play or pause",
    ),
    (
        "ui.player",
        r#"{"request":"stop"}"#,
        "Player: stop and let go",
    ),
    ("ui.media_step", r#"{"delta":1}"#, "Player: next track"),
    ("ui.media_step", r#"{"delta":-1}"#, "Player: previous track"),
    ("ui.properties", "{}", "Properties"),
    ("ui.inspect", r#"{"action":"hex"}"#, "Preview: hexadecimal"),
    (
        "ui.inspect",
        r#"{"action":"auto"}"#,
        "Preview: default presentation",
    ),
    ("ui.preview_find", "{}", "Preview: find text"),
    (
        "ui.preview_match",
        r#"{"delta":1}"#,
        "Preview: next search match",
    ),
    (
        "ui.preview_match",
        r#"{"delta":-1}"#,
        "Preview: previous search match",
    ),
    (
        "ui.inspect",
        r#"{"action":"source"}"#,
        "Preview: source / rendered",
    ),
    ("ui.wrap_lines", "{}", "Preview: wrap lines"),
    ("ui.preview_contents", "{}", "Preview: contents"),
    ("ui.preview_copy", "{}", "Preview: copy"),
    ("ui.preview_select_all", "{}", "Preview: select all"),
    ("ui.preview_close", "{}", "Preview: close"),
    (
        "ui.preview_zoom",
        r#"{"dir":"width"}"#,
        "Preview: fit width",
    ),
    (
        "ui.preview_zoom",
        r#"{"dir":"1"}"#,
        "Preview: actual size (100%)",
    ),
    (
        "ui.reader_continuous",
        "{}",
        "Document: toggle continuous scrolling",
    ),
    (
        "ui.preview_seek",
        r#"{"delta":-5}"#,
        "Preview: back five seconds",
    ),
    (
        "ui.preview_seek",
        r#"{"delta":5}"#,
        "Preview: forward five seconds",
    ),
    ("ui.preview_zoom", r#"{"dir":"in"}"#, "Preview: zoom in"),
    ("ui.preview_zoom", r#"{"dir":"out"}"#, "Preview: zoom out"),
    ("ui.preview_zoom", r#"{"dir":"reset"}"#, "Preview: fit"),
    ("ui.preview_fullscreen", "{}", "Preview: full screen"),
    ("ui.slideshow", "{}", "Preview: slideshow"),
    ("ui.map_open", "{}", "Map: open in Maps"),
    ("ui.peek", "{}", "Peek"),
    ("ui.search", "{}", "Search…"),
    ("ui.history_menu", "{}", "Back: history…"),
    (
        "ui.history_menu",
        r#"{"forward":true}"#,
        "Forward: history…",
    ),
    ("ui.jump", "{}", "Jump…"),
    ("ui.rename_bulk_prompt", "{}", "Rename several…"),
    ("ui.tags_prompt", "{}", "Tags…"),
    ("ui.timeline", "{}", "Timeline"),
    ("ui.split", "{}", "Split pane"),
    (
        "ui.split",
        r#"{"axis":"other"}"#,
        "Split pane the other way",
    ),
    (
        "ui.layout",
        r#"{"panes":"auto"}"#,
        "Panes: arrange automatically",
    ),
    ("ui.layout", r#"{"panes":"row"}"#, "Panes: side by side"),
    (
        "ui.layout",
        r#"{"panes":"column"}"#,
        "Panes: one above the other",
    ),
    (
        "ui.layout",
        r#"{"preview":"auto"}"#,
        "Preview: place automatically",
    ),
    (
        "ui.layout",
        r#"{"preview":"right"}"#,
        "Preview: on the right",
    ),
    (
        "ui.layout",
        r#"{"preview":"bottom"}"#,
        "Preview: at the bottom",
    ),
    (
        "ui.layout",
        r#"{"member":"auto"}"#,
        "File in an archive: place automatically",
    ),
    (
        "ui.layout",
        r#"{"member":"right"}"#,
        "File in an archive: on the right",
    ),
    (
        "ui.layout",
        r#"{"member":"bottom"}"#,
        "File in an archive: at the bottom",
    ),
    ("ui.focus_other_pane", "{}", "Focus other pane"),
    ("ui.quit", "{}", "Quit"),
];

/// [`UI_COMMANDS`] with their arguments parsed, once.
pub fn ui_commands() -> &'static [(&'static str, serde_json::Value, &'static str)] {
    static PARSED: OnceLock<Vec<(&'static str, serde_json::Value, &'static str)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        UI_COMMANDS
            .iter()
            .map(|(name, args, title)| {
                (
                    *name,
                    serde_json::from_str(args).expect("valid ui args"),
                    *title,
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
    pub title: String,
    /// Required argument names from the command's schema.
    pub required: Vec<String>,
}

impl Offer {
    fn of(name: &str, args: serde_json::Value, title: impl Into<String>) -> Self {
        Offer {
            name: name.to_owned(),
            args,
            title: title.into(),
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
            offers.push(Offer::of(&spec.name, form.args.clone(), form.title.clone()));
        }
    }
    for view in views {
        offers.push(Offer::of(
            "ui.view",
            serde_json::json!({ "id": view.id }),
            format!("{} view", view.title),
        ));
    }
    // Every setting is reachable by name, like a command: its page, its
    // section (a store of notes reads and writes), its title.
    let plugins = app.plugins.list();
    for spec in app.settings.schema() {
        if !spec.editable() {
            continue;
        }
        let mut path = vec![crate::settings::owner_title(spec.owner(), &plugins)];
        if !spec.section.is_empty() {
            path.push(spec.section.clone());
        }
        path.push(spec.title.clone());
        offers.push(Offer::of(
            "ui.settings",
            serde_json::json!({ "key": spec.key }),
            format!("Settings: {}", path.join(" › ")),
        ));
    }
    for (page, title) in [("plugins", "Plugins"), ("keyboard", "Keyboard shortcuts")] {
        offers.push(Offer::of(
            "ui.settings",
            serde_json::json!({ "page": page }),
            format!("Settings: {title}"),
        ));
    }
    for (name, args, title) in ui_commands() {
        offers.push(Offer::of(name, args.clone(), *title));
    }
    // A plugin's context-menu item that runs a shell command with its
    // own arguments (Open With… is `ui.choose` over the plugin's list)
    // has no command of its own to be listed by: list the item.
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
        offers.push(Offer::of(&command.name, command.args.clone(), item.title));
    }
    offers
}

impl Shell {
    fn palette_items(&self) -> Vec<PaletteItem> {
        let sorts: Vec<SortKey> = self.sort_keys().into_iter().map(|(_, key)| key).collect();
        offered(&self.app, &self.available_views(), &sorts)
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
            let mut input = TextInput::new("Type a command", cx);
            input.set_label("Command");
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
                let asked = format!("{} · {}", args.name, args.required[args.index]);
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
            let asked = format!("{} · {}", item.name, item.required[0]);
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

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.palette_refilter(cx);
    let p = shell.palette.as_ref()?;
    let theme = cx.global::<Theme>().clone();

    let label = match &p.args {
        Some(a) => format!("{} · {}", a.name, a.required[a.index]),
        None => "Command".to_owned(),
    };
    let mut panel = modal::panel("palette", 560., &theme)
        .aria_label("Command palette")
        .max_h(px(420.))
        .key_context(keymap::PALETTE)
        .child(
            div()
                .p_2()
                .flex()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_xs().text_color(theme.text_muted).child(label))
                .child(div().flex_1().child(p.input.clone())),
        );
    if p.args.is_none() {
        // The commands found, a list box beside the field that keeps the
        // keyboard: the highlighted one is said as focused while the field
        // has the focus.
        let a11y = shell.a11y;
        let typing = gpui::Focusable::focus_handle(p.input.read(cx), cx);
        let mut list = modal::options("palette-results", "Commands", p.filtered.len(), a11y)
            .flex()
            .flex_col()
            .py_1()
            .overflow_hidden();
        for row_ix in p.picker.shown(p.filtered.len()) {
            let item = &p.items[p.filtered[row_ix]];
            let hl = row_ix == p.picker.highlighted;
            let muted = if hl {
                theme.selection_text
            } else {
                theme.text_muted
            };
            let name = item.name.clone();
            let option = modal::option(
                div().id(("cmd", row_ix)),
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
                    .h(px(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .when(hl, |d| {
                        d.bg(theme.selection_bg).text_color(theme.selection_text)
                    })
                    .when(!hl, |d| d.hover(|s| s.bg(theme.hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(p) = &mut this.palette {
                            p.picker.highlighted = row_ix;
                        }
                        this.palette_submit(window, cx);
                    }))
                    .child(div().child(item.title.clone()))
                    .child(div().text_xs().text_color(muted).child(name))
                    .child(div().flex_1())
                    .children(
                        item.keys
                            .clone()
                            .map(|k| div().text_xs().text_color(muted).child(k)),
                    ),
            );
        }
        if p.filtered.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(theme.text_muted)
                    .child("No matching command"),
            );
        }
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

    fn titles(app: &App, sorts: &[SortKey]) -> Vec<String> {
        super::offered(app, &[], sorts)
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
                said: "Copied".into(),
            })
        };
        app.commands
            .register::<NoArgs, _>("test", "test.copy", "Copy a test text", "", move |_, _| {
                copied()
            })
            .reads();
        app.commands
            .register::<NoArgs, _>("test", "test.rows", "Rows of a test panel", "", |_, _| {
                Ok(Outcome::Value(serde_json::json!([])))
            })
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
