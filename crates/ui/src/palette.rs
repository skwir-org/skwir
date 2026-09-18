//! Command palette: fuzzy search over every command, run on Enter, prompt for
//! required string arguments in sequence.

use gpui::{
    AnyElement, Context, Entity, MouseButton, Subscription, Window, deferred, div, prelude::*, px,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::input::{InputEvent, TextInput};
use crate::keymap;
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
    pub highlighted: usize,
    query: Option<String>,
    pub args: Option<ArgPrompt>,
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
    ("ui.view", r#"{"id":"list"}"#, "View: List"),
    ("ui.view", r#"{"id":"grid"}"#, "View: Grid"),
    ("ui.view", r#"{"id":"columns"}"#, "View: Columns"),
    ("ui.sort", r#"{"key":"name"}"#, "Sort by name"),
    ("ui.sort", r#"{"key":"size"}"#, "Sort by size"),
    ("ui.sort", r#"{"key":"modified"}"#, "Sort by date modified"),
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
    ("ui.reader_open_external", "{}", "Document: open externally"),
    (
        "ui.preview_zoom",
        r#"{"dir":"width"}"#,
        "Document: fit width",
    ),
    (
        "ui.preview_zoom",
        r#"{"dir":"1"}"#,
        "Preview: actual size (100%)",
    ),
    ("ui.reader_find", "{}", "Document: find text"),
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
    ("ui.usage", "{}", "Disk usage"),
    ("ui.timeline", "{}", "Timeline"),
    ("ui.split", "{}", "Split pane"),
    ("ui.focus_other_pane", "{}", "Focus other pane"),
    ("ui.shelf_open", "{}", "Open shelf"),
    ("ui.quit", "{}", "Quit"),
];

impl Shell {
    fn palette_items(&self) -> Vec<PaletteItem> {
        let key_of = |name: &str| {
            self.key_hints
                .iter()
                .find(|h| h.command == name)
                .map(|h| h.keys.clone())
        };
        let mut items: Vec<PaletteItem> = self
            .app
            .commands
            .list()
            .into_iter()
            // Structured preference batches are edited by the Settings dialog.
            .filter(|c| &*c.name != "settings.apply")
            .map(|c| PaletteItem {
                keys: key_of(&c.name),
                name: c.name.to_string(),
                args: serde_json::json!({}),
                title: c.title.clone(),
                required: c.required_args(),
            })
            .collect();
        for (name, args, title) in UI_COMMANDS {
            items.push(PaletteItem {
                name: (*name).into(),
                args: serde_json::from_str(args).expect("valid ui args"),
                title: (*title).into(),
                keys: key_of(name),
                required: Vec::new(),
            });
        }
        items
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            highlighted: 0,
            query: None,
            args: None,
            _sub: sub,
        });
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        window.focus(&self.ui().focus);
        cx.notify();
    }

    pub fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.palette
            && !p.filtered.is_empty()
        {
            let n = p.filtered.len() as isize;
            p.highlighted = ((p.highlighted as isize + delta).rem_euclid(n)) as usize;
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
        let Some(&ix) = p.filtered.get(p.highlighted) else {
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
        p.highlighted = 0;
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
    let mut panel = div()
        .w(px(560.))
        .max_h(px(420.))
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_lg()
        .key_context(keymap::PALETTE)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
        for (row_ix, &ix) in p.filtered.iter().take(14).enumerate() {
            let item = &p.items[ix];
            let hl = row_ix == p.highlighted;
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(p) = &mut this.palette {
                            p.highlighted = row_ix;
                        }
                        this.palette_submit(window, cx);
                    }))
                    .child(div().child(item.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if hl {
                                theme.selection_text
                            } else {
                                theme.text_muted
                            })
                            .child(name),
                    )
                    .child(div().flex_1())
                    .children(item.keys.clone().map(|k| {
                        div()
                            .text_xs()
                            .text_color(if hl {
                                theme.selection_text
                            } else {
                                theme.text_muted
                            })
                            .child(k)
                    })),
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
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(80.))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(panel),
        )
        .with_priority(4)
        .into_any_element(),
    )
}
