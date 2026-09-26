//! Command palette: fuzzy search over every command, run on Enter, prompt for
//! required string arguments in sequence.

use std::sync::OnceLock;

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

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

/// The shell's own commands, shown alongside the core's.
pub const UI_COMMANDS: &[(&str, &str, &str)] = &[
    ("ui.open", "{}", "Open"),
    ("ui.focus_address", "{}", "Focus address bar"),
    ("ui.focus_pane", "{}", "Focus file list"),
    ("ui.palette", "{}", "Command palette"),
    ("ui.settings", "{}", "Settings…"),
    ("ui.rename_prompt", "{}", "Rename…"),
    ("ui.mkdir_prompt", "{}", "New folder…"),
    ("ui.confirm_delete", "{}", "Delete permanently…"),
    ("ui.toggle_hidden", "{}", "Toggle hidden files"),
    ("ui.toggle_sidebar", "{}", "Toggle sidebar"),
    ("ui.open_in_tab", "{}", "Open in new tab"),
    ("ui.sort", r#"{"key":"name"}"#, "Sort by name"),
    ("ui.sort", r#"{"key":"size"}"#, "Sort by size"),
    ("ui.sort", r#"{"key":"modified"}"#, "Sort by date modified"),
    ("ui.grid_fill", "{}", "Toggle filled thumbnails"),
    ("ui.full_width", "{}", "Toggle full-width reading"),
    ("ui.zoom", r#"{"dir":"in"}"#, "Zoom in"),
    ("ui.zoom", r#"{"dir":"out"}"#, "Zoom out"),
    ("ui.zoom", r#"{"dir":"reset"}"#, "Zoom reset"),
    ("ui.toggle_preview", "{}", "Toggle preview panel"),
    (
        "ui.preview_page",
        r#"{"delta":-1}"#,
        "Preview: previous page",
    ),
    ("ui.preview_page", r#"{"delta":1}"#, "Preview: next page"),
    ("ui.reader_page_edit", "{}", "Document: go to page…"),
    (
        "ui.inspect",
        r#"{"action":"section_edit"}"#,
        "Book: go to chapter…",
    ),
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
    ("ui.preview_details", "{}", "Preview: show or hide details"),
    ("ui.inspect", r#"{"action":"hex"}"#, "Preview: hexadecimal"),
    (
        "ui.inspect",
        r#"{"action":"auto"}"#,
        "Preview: default presentation",
    ),
    (
        "ui.inspect",
        r#"{"action":"search"}"#,
        "Preview: find content",
    ),
    (
        "ui.inspect",
        r#"{"action":"source"}"#,
        "Preview: source / rendered",
    ),
    (
        "ui.inspect",
        r#"{"action":"outline"}"#,
        "Preview: headings and contents",
    ),
    (
        "ui.inspect",
        r#"{"action":"copy"}"#,
        "Preview: copy content",
    ),
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
    ("ui.reader_find", "{}", "Document: find text"),
    (
        "ui.reader_continuous",
        "{}",
        "Document: toggle continuous scrolling",
    ),
    ("ui.reader_outline", "{}", "Document: contents"),
    ("ui.reader_copy", "{}", "Document: copy selected text"),
    ("ui.reader_select_all", "{}", "Document: select page text"),
    (
        "ui.reader_match",
        r#"{"delta":1}"#,
        "Document: next search match",
    ),
    (
        "ui.reader_match",
        r#"{"delta":-1}"#,
        "Document: previous search match",
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
    ("ui.peek", "{}", "Peek"),
    ("ui.search", "{}", "Search…"),
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
    ("ui.focus_other_pane", "{}", "Focus other pane"),
    ("ui.shelf_open", "{}", "Open shelf"),
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

impl Shell {
    fn palette_items(&self) -> Vec<PaletteItem> {
        let no_args = serde_json::json!({});
        let mut items: Vec<PaletteItem> = self
            .app
            .commands
            .list()
            .into_iter()
            // Structured preference batches are edited by the Settings dialog.
            .filter(|c| !matches!(&*c.name, "settings.apply" | "plugins.set_enabled"))
            .map(|c| PaletteItem {
                keys: self.keys_for(&c.name, &no_args),
                name: c.name.to_string(),
                args: serde_json::json!({}),
                title: c.title.clone(),
                required: c.required_args(),
            })
            .collect();
        for spec in self.available_views() {
            items.push(PaletteItem {
                name: "ui.view".into(),
                args: serde_json::json!({"id": spec.id}),
                title: format!("{} view", spec.title),
                keys: self.keys_for("ui.view", &serde_json::json!({ "id": spec.id })),
                required: Vec::new(),
            });
        }
        // Every setting is reachable by name, like a command.
        let plugins = self.app.plugins.list();
        for spec in self.app.settings.schema() {
            if !spec.editable() {
                continue;
            }
            let owner = match spec.owner() {
                crate::settings::CORE_OWNER => "General".to_owned(),
                crate::settings::UI_OWNER => "Appearance".to_owned(),
                id => plugins
                    .iter()
                    .find(|plugin| plugin.id == id)
                    .map_or_else(|| id.to_owned(), |plugin| plugin.title.clone()),
            };
            items.push(PaletteItem {
                name: "ui.settings".into(),
                args: serde_json::json!({ "key": spec.key }),
                title: format!("Settings: {owner} › {}", spec.title),
                keys: None,
                required: Vec::new(),
            });
        }
        for (page, title) in [("plugins", "Plugins"), ("keyboard", "Keyboard shortcuts")] {
            items.push(PaletteItem {
                name: "ui.settings".into(),
                args: serde_json::json!({ "page": page }),
                title: format!("Settings: {title}"),
                keys: None,
                required: Vec::new(),
            });
        }
        for (name, args, title) in ui_commands() {
            items.push(PaletteItem {
                name: (*name).into(),
                args: args.clone(),
                title: (*title).into(),
                keys: self.keys_for(name, args),
                required: Vec::new(),
            });
        }
        // A plugin's context-menu item that runs a shell command with its
        // own arguments (Open With… is `ui.choose` over the plugin's list)
        // has no command of its own to be listed by: list the item.
        for item in self.app.registry.menus.all() {
            let command = &item.command;
            if item.menu != "context"
                || !command.name.starts_with("ui.")
                || items
                    .iter()
                    .any(|i| *i.name == *command.name && i.args == command.args)
            {
                continue;
            }
            items.push(PaletteItem {
                keys: self.keys_for(&command.name, &command.args),
                name: command.name.to_string(),
                args: command.args.clone(),
                title: item.title,
                required: Vec::new(),
            });
        }
        items
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new("Type a command", cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.palette_submit(window, cx),
                InputEvent::Cancel => this.close_palette(window, cx),
                InputEvent::Changed => cx.notify(),
            },
        );
        input.update(cx, |i, _| i.focus(window));
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
            palette.back.restore(window, &self.ui().focus);
        }
        cx.notify();
    }

    pub fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.palette {
            p.picker.step(delta, p.filtered.len());
            cx.notify();
        }
    }

    fn palette_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else { return };
        if let Some(args) = &mut p.args {
            // Collecting required arguments one at a time.
            let value = p.input.read(cx).text().to_owned();
            let key = args.required[args.index].clone();
            args.values.insert(key, serde_json::Value::String(value));
            args.index += 1;
            if args.index < args.required.len() {
                p.input.update(cx, |i, cx| i.set_text("", cx));
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
            p.args = Some(ArgPrompt {
                name: item.name,
                required: item.required,
                values: serde_json::Map::new(),
                index: 0,
            });
            p.input.update(cx, |i, cx| i.set_text("", cx));
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
        if q.trim().is_empty() {
            p.filtered = (0..p.items.len()).collect();
        } else {
            let mut matcher = Matcher::new(Config::DEFAULT);
            let pattern = Pattern::parse(&q, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let mut scored: Vec<(u32, usize)> = p
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| {
                    let hay = format!("{} {}", it.title, it.name);
                    pattern
                        .score(Utf32Str::new(&hay, &mut buf), &mut matcher)
                        .map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            p.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
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
    let mut panel = modal::panel(560., &theme)
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
        let mut list = div().flex().flex_col().py_1().overflow_hidden();
        for row_ix in p.picker.shown(p.filtered.len()) {
            let item = &p.items[p.filtered[row_ix]];
            let hl = row_ix == p.picker.highlighted;
            let muted = if hl {
                theme.selection_text
            } else {
                theme.text_muted
            };
            let name = item.name.clone();
            list = list.child(
                div()
                    .id(("cmd", row_ix))
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
