//! Shared native viewers for optional structured preview plugins.
use crate::{
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use bibli_core::inspection::{InspectRequest, Inspection, InspectionKind};
use gpui::{
    AnyElement, Context, ListAlignment, ListState, RenderImage, Window, div, prelude::*, px,
};
use std::{collections::BTreeSet, sync::Arc};
pub struct Display {
    pub data: Arc<Inspection>,
    pub images: Vec<Option<Arc<RenderImage>>>,
}
impl Display {
    pub fn new(mut data: Inspection, dark: bool) -> Self {
        data.blocks = bibli_core::inspection::split_blocks(data.blocks);
        for block in &mut data.blocks {
            if !block.code {
                continue;
            }
            let syntaxes = crate::preview::syntaxes();
            let syntax = block
                .language
                .as_deref()
                .and_then(|language| syntaxes.find_syntax_by_token(language))
                .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
            let theme = &crate::preview::themes().themes[if dark {
                "base16-ocean.dark"
            } else {
                "base16-ocean.light"
            }];
            let mut highlight = syntect::easy::HighlightLines::new(syntax, theme);
            let mut offset = 0;
            block.runs.clear();
            for line in syntect::util::LinesWithEndings::from(&block.text) {
                if let Ok(ranges) = highlight.highlight_line(line, syntaxes) {
                    for (style, text) in ranges {
                        block.runs.push(bibli_core::inspection::Run {
                            range: offset..offset + text.len(),
                            color: Some([
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            ]),
                            bold: false,
                            italic: false,
                            code: true,
                            link: None,
                        });
                        offset += text.len();
                    }
                } else {
                    offset += line.len();
                }
            }
        }
        let images = std::iter::once(data.image.take())
            .chain(data.blocks.iter_mut().map(|b| b.image.take()))
            .map(|i| i.and_then(|i| crate::thumbs::to_render_image(&i)))
            .collect();
        Self {
            data: Arc::new(data),
            images,
        }
    }
}
pub struct State {
    pub focus: Option<gpui::FocusHandle>,
    pub request: InspectRequest,
    list: ListState,
    visible: Arc<Vec<usize>>,
    collapsed: BTreeSet<usize>,
    selected: Option<usize>,
    selection: Option<crate::inspector_text::Selection>,
    dragging: bool,
    font_size: f32,
    outline: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            request: InspectRequest::default(),
            focus: None,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            visible: Arc::new(Vec::new()),
            collapsed: BTreeSet::new(),
            selected: None,
            selection: None,
            dragging: false,
            font_size: 16.,
            outline: false,
        }
    }
}
impl State {
    pub fn reset(&mut self, data: &Inspection) {
        self.collapsed.clear();
        self.selected = None;
        self.selection = None;
        self.dragging = false;
        self.outline = false;
        self.reindex(data);
    }
    fn reindex(&mut self, data: &Inspection) {
        let mut hidden = None;
        self.visible = Arc::new(
            data.rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    if hidden.is_some_and(|d| r.depth > d) {
                        return None;
                    }
                    hidden = None;
                    if self.collapsed.contains(&i) {
                        hidden = Some(r.depth);
                    }
                    Some(i)
                })
                .collect(),
        );
        self.list.reset(data.blocks.len() + self.visible.len() + 1);
    }
}
impl Shell {
    pub fn inspect_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = args["action"].as_str().unwrap_or("auto");
        if action == "search" || action == "offset" {
            let pane = self.pane;
            let generation = self.preview.generation;
            let kind = if action == "search" {
                crate::prompts::PromptKind::InspectSearch { pane, generation }
            } else {
                crate::prompts::PromptKind::InspectJump { pane, generation }
            };
            self.open_prompt(
                kind,
                if action == "search" {
                    "Find in preview"
                } else {
                    "Jump to byte offset"
                },
                "",
                None,
                window,
                cx,
            );
            return;
        }
        if let Some(PreviewContent::Inspection(display)) = &self.preview.content {
            match action {
                "text_down" | "text_move" => {
                    if action == "text_move" && !self.preview.inspection.dragging {
                        return;
                    }
                    let index = args["index"].as_u64().unwrap_or(0) as usize;
                    if let Some(block) = display.data.blocks.get(index) {
                        let mut offset =
                            (args["offset"].as_u64().unwrap_or(0) as usize).min(block.text.len());
                        while !block.text.is_char_boundary(offset) {
                            offset -= 1;
                        }
                        let state = &mut self.preview.inspection;
                        state.selected = Some(index);
                        if action == "text_down" {
                            if let Some(focus) = &state.focus {
                                window.focus(focus);
                            }
                            state.selection = Some(crate::inspector_text::Selection {
                                start: (index, offset),
                                end: (index, offset),
                            });
                            state.dragging = true;
                        } else if let Some(selection) = &mut state.selection {
                            selection.end = (index, offset);
                        }
                        cx.notify();
                    }
                    return;
                }
                "text_up" => {
                    self.preview.inspection.dragging = false;
                    return;
                }
                "copy" => {
                    if let Some(text) = self
                        .preview
                        .inspection
                        .selection
                        .and_then(|s| s.text(&display.data.blocks))
                    {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        return;
                    }

                    let text = self
                        .preview
                        .inspection
                        .selected
                        .and_then(|i| {
                            display
                                .data
                                .blocks
                                .get(i)
                                .map(|b| b.text.clone())
                                .or_else(|| {
                                    display
                                        .data
                                        .rows
                                        .get(i.saturating_sub(display.data.blocks.len()))
                                        .map(|r| r.cells.join("\t"))
                                })
                        })
                        .unwrap_or_else(|| display.data.plain_text());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    return;
                }
                "select" => {
                    self.preview.inspection.selected = args["index"].as_u64().map(|i| i as usize);
                    cx.notify();
                    return;
                }
                "collapse" => {
                    if let Some(i) = args["index"].as_u64() {
                        let i = i as usize;
                        if !self.preview.inspection.collapsed.remove(&i) {
                            self.preview.inspection.collapsed.insert(i);
                        }
                        self.preview.inspection.reindex(&display.data);
                    }
                    cx.notify();
                    return;
                }
                "larger" | "smaller" => {
                    let state = &mut self.preview.inspection;
                    state.font_size = (state.font_size + if action == "larger" { 2. } else { -2. })
                        .clamp(10., 40.);
                    state.list.reset(state.list.item_count());
                    cx.notify();
                    return;
                }
                "outline" => {
                    self.preview.inspection.outline = !self.preview.inspection.outline;
                    cx.notify();
                    return;
                }
                "block" => {
                    if let Some(i) = args["index"].as_u64() {
                        self.preview
                            .inspection
                            .list
                            .scroll_to_reveal_item(i as usize + 1);
                    }
                    cx.notify();
                    return;
                }
                "find"
                    if self.preview.inspection.request.provider.as_deref()
                        != Some("preview-hex")
                        && display.data.kind != InspectionKind::Hex
                        && display.data.kind != InspectionKind::Font
                        && display.data.kind != InspectionKind::Book =>
                {
                    let query = args["value"].as_str().unwrap_or("").to_lowercase();
                    let found = display
                        .data
                        .blocks
                        .iter()
                        .position(|b| b.text.to_lowercase().contains(&query))
                        .or_else(|| {
                            display
                                .data
                                .rows
                                .iter()
                                .position(|r| r.cells.join("\t").to_lowercase().contains(&query))
                                .map(|i| display.data.blocks.len() + i)
                        });
                    if let Some(i) = found {
                        self.preview.inspection.selected = Some(i);
                        self.preview.inspection.collapsed.clear();
                        self.preview.inspection.reindex(&display.data);
                        self.preview.inspection.list.scroll_to_reveal_item(i + 1);
                    } else {
                        self.set_status("No match in this preview section", false, cx);
                    }
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }
        if action == "link" {
            if self
                .preview
                .inspection
                .selection
                .is_some_and(|s| s.start != s.end)
            {
                return;
            }
            if let Some(anchor) = args["target"].as_str().and_then(|s| s.strip_prefix('#')) {
                if let Some(PreviewContent::Inspection(display)) = &self.preview.content
                    && let Some(i) = display
                        .data
                        .blocks
                        .iter()
                        .position(|b| b.anchor.as_deref() == Some(anchor))
                {
                    self.preview.inspection.list.scroll_to_reveal_item(i + 1);
                    cx.notify();
                }
                return;
            }
            let href = args["target"].as_str().unwrap_or("");
            if href.starts_with("https://")
                || href.starts_with("http://")
                || href.starts_with("mailto:")
            {
                cx.open_url(href);
                return;
            }
            if !self
                .preview
                .info
                .name
                .to_ascii_lowercase()
                .ends_with(".epub")
            {
                self.set_status("Only web links can be opened from this preview", false, cx);
                return;
            }
        }
        let state = &mut self.preview.inspection;
        state.request.query = None;
        match action {
            "hex" => {
                if !self.app.previews.ids().contains(&"preview-hex") {
                    return;
                }
                state.request = InspectRequest {
                    provider: Some("preview-hex".into()),
                    ..InspectRequest::default()
                };
            }
            "auto" => state.request = InspectRequest::default(),
            "source" => {
                state.request.source = !state.request.source;
                state.request.offset = 0;
            }
            "page" => {
                state.request.offset = args["offset"].as_u64().unwrap_or(0);
                state.request.target = None;
            }
            "target" | "link" => {
                state.request.target = args["target"].as_str().map(str::to_owned);
                state.request.offset = 0;
            }
            "back" => {
                state.request.target = None;
                state.request.offset = 0;
            }
            "find" => {
                state.request.query = args["value"].as_str().map(str::to_owned);
            }
            "jump" => {
                let value = args["value"].as_str().unwrap_or("");
                let parsed = if let Some(hex) = value.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16)
                } else {
                    value.parse()
                };
                if let Ok(n) = parsed {
                    state.request.offset = n;
                } else {
                    self.set_status("Enter a decimal or hexadecimal offset", true, cx);
                    return;
                }
            }
            _ => return,
        }
        self.cancel_pending_preview();
        self.media_exit_fullscreen(window, cx);
        self.preview.media = crate::media::MediaState::default();
        self.clear_document_pages(window, cx);
        self.preview.reader = crate::reader::ReaderState::default();
        self.preview.phase = PreviewPhase::Queued;
        self.preview.menu = None;
        cx.notify();
    }
}
fn button(id: &'static str, icon: &'static str, title: &'static str, action: &str) -> Button {
    Button::new(id, icon, title, "ui.inspect").args(serde_json::json!({"action":action}))
}
pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let Some(PreviewContent::Inspection(display)) = &shell.preview.content else {
        return div().into_any_element();
    };
    let anchor = crate::elements::popover::PopoverAnchor::default();
    let variants = (0..3)
        .map(|mode| toolbar_row(shell, &display.data, theme, mode, &anchor, cx))
        .collect();
    let mut root = div()
        .relative()
        .flex_none()
        .child(crate::elements::adaptive::AdaptiveRow {
            variants,
            height: 42.,
        });
    if let Some(popup) = &shell.preview.menu {
        let mut panel = toolbar::popup_panel(popup, "ui.preview_menu", theme, cx);
        for (id, icon, title, action) in tools(&display.data) {
            if action == "hex" && !shell.app.previews.ids().contains(&"preview-hex") {
                continue;
            }
            panel = panel.child(
                button(id, icon, title, action)
                    .label(title)
                    .render(shell, cx)
                    .justify_start(),
            );
        }
        panel = panel.child(
            Button::new(
                "inspect-external",
                "open",
                "Open externally",
                "ui.preview_open_external",
            )
            .label("Open externally")
            .render(shell, cx),
        );
        root = root.child(anchor.popup(panel));
    }
    root.into_any_element()
}
fn tools(data: &Inspection) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let mut tools = vec![
        ("inspect-auto", "back", "Default presentation", "auto"),
        ("inspect-hex", "grid", "View as hexadecimal", "hex"),
        ("inspect-search", "search", "Find in preview", "search"),
        ("inspect-copy", "copy", "Copy selection or section", "copy"),
    ];
    if data.source_available {
        tools.push(("inspect-source", "list", "Source / rendered", "source"));
    }
    if data.kind == InspectionKind::Hex {
        tools.push(("inspect-offset", "move", "Jump to byte offset", "offset"));
    }
    if !data.blocks.is_empty() {
        tools.extend([
            ("inspect-smaller", "minus", "Smaller text", "smaller"),
            ("inspect-larger", "plus", "Larger text", "larger"),
            (
                "inspect-outline",
                "contents",
                "Headings and contents",
                "outline",
            ),
        ]);
    }
    tools
}
fn toolbar_row(
    shell: &Shell,
    data: &Inspection,
    theme: &Theme,
    mode: usize,
    anchor: &crate::elements::popover::PopoverAnchor,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut row = toolbar::row(theme).id(("inspect-tools", mode));
    if shell.preview.inspection.request.target.is_some() {
        row = row
            .child(button("inspect-parent", "up", "Container contents", "back").render(shell, cx));
    }
    for (id, icon, title, offset) in [
        ("inspect-prev", "left", "Previous section", data.previous),
        ("inspect-next", "right", "Next section", data.next),
    ] {
        if let Some(offset) = offset {
            row = row.child(
                Button::new(id, icon, title, "ui.inspect")
                    .args(serde_json::json!({"action":"page","offset":offset}))
                    .render(shell, cx),
            );
        }
    }
    for (id, icon, title, action) in tools(data) {
        if (mode == 1 && !matches!(action, "source" | "search" | "copy" | "outline"))
            || (mode == 2 && action != "search")
        {
            continue;
        }
        if action == "hex" && !shell.app.previews.ids().contains(&"preview-hex") {
            continue;
        }
        row = row.child(
            button(id, icon, title, action)
                .active(action == "source" && shell.preview.inspection.request.source)
                .render(shell, cx),
        );
    }
    if mode == 0 {
        row = row.child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .child(data.title.clone()),
        );
    }
    row.child(div().flex_1().min_w_0())
        .child(
            Button::new(
                "inspect-more",
                "more",
                "More preview controls",
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind":"inspection"}))
            .active(shell.preview.menu.is_some())
            .anchor_if(anchor, true)
            .render(shell, cx),
        )
        .into_any_element()
}
pub fn content(
    shell: &Shell,
    display: &Display,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let data = display.data.clone();
    let images = display.images.clone();
    let visible = shell.preview.inspection.visible.clone();
    let collapsed = shell.preview.inspection.collapsed.clone();
    let selected = shell.preview.inspection.selected;
    let selection = shell.preview.inspection.selection;
    let font_size = shell.preview.inspection.font_size;
    let mono = shell.mono_font.clone();
    let pane = shell.pane;
    let theme = theme.clone();
    let entity = cx.entity().downgrade();
    let list = gpui::list(shell.preview.inspection.list.clone(), move |index, _, _| {
        if index == 0 {
            return div()
                .p_2()
                .text_color(theme.text_muted)
                .child(data.notice.clone())
                .when_some(images[0].as_ref(), |d, image| {
                    d.child(gpui::img(image.clone()).max_w_full())
                })
                .into_any_element();
        }
        let i = index - 1;
        let (logical, mut row) = if let Some(block) = data.blocks.get(i) {
            let text =
                crate::inspector_text::render(block, i, selection, &theme, pane, entity.clone());
            (
                i,
                div()
                    .id(("inspect-block", i))
                    .p_2()
                    .w_full()
                    .text_size(px(if block.heading > 0 {
                        font_size + f32::from(7 - block.heading) * 2.
                    } else {
                        font_size
                    }))
                    .when(block.heading > 0, |d| d.font_weight(gpui::FontWeight::BOLD))
                    .when(block.code, |d| {
                        d.font_family(mono.clone()).text_size(px(font_size - 2.))
                    })
                    .when(block.quote, |d| {
                        d.border_l_2().border_color(theme.accent).pl_4()
                    })
                    .child(text)
                    .when_some(images[i + 1].as_ref(), |d, image| {
                        d.child(gpui::img(image.clone()).max_w_full())
                    }),
            )
        } else {
            let ri = visible[i - data.blocks.len()];
            let r = &data.rows[ri];
            let weak = entity.clone();
            let target = r.target.clone();
            let branch = r.branch;
            let mut row = div()
                .id(("inspect-row", ri))
                .p_2()
                .w_full()
                .flex()
                .gap_3()
                .text_sm()
                .pl(px(8. + r.depth.min(32) as f32 * 14.))
                .when(data.kind == InspectionKind::Hex, |d| {
                    d.font_family(mono.clone()).whitespace_nowrap()
                });
            if branch {
                row = row.child(if collapsed.contains(&ri) {
                    "▸"
                } else {
                    "▾"
                });
            }
            for (column, cell) in r.cells.iter().enumerate() {
                row = row.child(
                    div()
                        .w(px(column_width(data.kind, column)))
                        .flex_none()
                        .child(cell.chars().take(512).collect::<String>()),
                );
            }
            let logical_row = data.blocks.len() + ri;
            row = row.cursor_pointer().on_click(move |_, window, cx| {
                let action = if branch {
                    "collapse"
                } else if target.is_some() {
                    "target"
                } else {
                    "select"
                };
                let index = if branch { ri } else { logical_row };
                let _ = weak.update(cx, |s, cx| {
                    s.with_pane(pane, |s| {
                        s.run(
                            "ui.inspect",
                            serde_json::json!({"action":action,"index":index,"target":target}),
                            window,
                            cx,
                        );
                    });
                });
            });
            (data.blocks.len() + ri, row)
        };
        if selected == Some(logical) && logical >= data.blocks.len() {
            row = row.bg(theme.selection_bg);
        }
        row.into_any_element()
    })
    .size_full();
    let mut body = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .key_context("Inspector")
        .when_some(shell.preview.inspection.focus.as_ref(), |d, focus| {
            d.track_focus(focus)
        });
    if shell.preview.inspection.outline {
        let mut outline = div().flex().flex_wrap().gap_1();
        for (i, b) in display
            .data
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.heading > 0)
        {
            outline = outline.child(
                Button::new(
                    format!("inspect-heading-{i}"),
                    "",
                    b.text.clone(),
                    "ui.inspect",
                )
                .label(b.text.clone())
                .args(serde_json::json!({"action":"block","index":i}))
                .render(shell, cx),
            );
        }
        if display.data.kind == InspectionKind::Book {
            for (i, row) in display.data.rows.iter().enumerate() {
                if let Some(target) = &row.target {
                    outline = outline.child(
                        Button::new(
                            format!("inspect-chapter-{i}"),
                            "",
                            row.cells.join(" "),
                            "ui.inspect",
                        )
                        .label(row.cells.join(" "))
                        .args(serde_json::json!({"action":"target","target":target}))
                        .render(shell, cx),
                    );
                }
            }
        }
        body = body.child(
            div()
                .id("inspect-contents")
                .max_h(px(200.))
                .overflow_y_scroll()
                .child(outline),
        );
    }
    if !display.data.columns.is_empty() {
        let mut header = div().p_2().flex().gap_3().text_xs();
        for (index, column) in display.data.columns.iter().enumerate() {
            header = header.child(
                div()
                    .w(px(column_width(display.data.kind, index)))
                    .flex_none()
                    .child(column.clone()),
            );
        }
        body = body.child(header);
    }
    let body = body.child(div().flex_1().min_h_0().child(list));
    if matches!(
        display.data.kind,
        InspectionKind::Table | InspectionKind::Hex
    ) {
        let width = (0..display.data.columns.len())
            .map(|i| column_width(display.data.kind, i) + 12.)
            .sum::<f32>()
            + 16.;
        div()
            .id("inspection-horizontal")
            .flex_1()
            .min_h_0()
            .overflow_x_scroll()
            .child(body.w(px(width)).min_w_full().h_full())
            .into_any_element()
    } else {
        body.into_any_element()
    }
}

fn column_width(kind: InspectionKind, index: usize) -> f32 {
    if kind == InspectionKind::Hex {
        [160., 460., 170.].get(index).copied().unwrap_or(160.)
    } else if kind == InspectionKind::Table {
        160.
    } else {
        140.
    }
}
