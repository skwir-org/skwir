//! Rendering of the Settings dialog (state and behaviour: `settings.rs`).
//!
//! The page is a flat list of children inside one scroll container, so a
//! keyboard stop can be brought into view by its child index. Cards are drawn
//! by giving the first and last row of a group their rounded corners.

use bibli_core::plugin::PluginInfo;
use bibli_core::settings::{SettingKind, SettingSpec};
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, MouseButton, SharedString, Window, div,
    prelude::*, px, relative,
};

use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::input::TextInput;
use crate::keymap::KeyHint;
use crate::settings::{CORE_OWNER, Page, Row, SettingsEvent, SettingsView, Stop, UI_OWNER};
use crate::theme::Theme;

/// Segmented controls hold this many choices; longer lists use a dropdown.
const SEGMENTED_MAX: usize = 4;
const TEXT_WIDTH: f32 = 260.;

/// Where a row sits in its card.
#[derive(Clone, Copy)]
struct Edge {
    first: bool,
    last: bool,
}

/// What a setting row needs, detached from `self` so controls can borrow it.
struct Snapshot {
    spec: SettingSpec,
    value: String,
    input: Option<Entity<TextInput>>,
    anchor: PopoverAnchor,
    modified: bool,
    default_text: String,
}

impl Snapshot {
    fn of(row: &Row) -> Self {
        Snapshot {
            spec: row.spec.clone(),
            value: row.value.clone(),
            input: row.input.clone(),
            anchor: row.anchor.clone(),
            modified: row.modified(),
            default_text: row.default_text(),
        }
    }
}

fn matches(query: &str, haystack: &[&str]) -> bool {
    haystack
        .iter()
        .any(|text| text.to_lowercase().contains(query))
}

fn spec_matches(query: &str, spec: &SettingSpec, owner_title: &str) -> bool {
    matches(
        query,
        &[
            &spec.title,
            &spec.description,
            &spec.key,
            &spec.section,
            owner_title,
        ],
    ) || spec
        .constraints
        .choice_labels
        .iter()
        .any(|label| label.to_lowercase().contains(query))
}

impl SettingsView {
    fn command_title(&self, hint: &KeyHint) -> String {
        crate::keymap::title_of(&self.app, hint)
    }

    fn stop(&mut self, focus: &gpui::FocusHandle, item: Option<usize>, key: Option<&str>) {
        self.stops.push(Stop {
            focus: focus.clone(),
            item,
            key: key.map(str::to_owned),
        });
    }

    // ------------------------------------------------------------ sidebar

    fn render_sidebar(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search_focused = self.search.read(cx).is_focused(window);
        let searching = !self.query(cx).is_empty();
        let mut list = div()
            .id("settings-nav")
            .track_focus(&self.nav)
            .key_context("SettingsNav")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p_2();
        let nav_focused = self.nav.is_focused(window);
        let mut plugins_caption = false;
        for page in self.pages() {
            let icon = match &page {
                Page::General => "sliders",
                Page::Appearance => "appearance",
                Page::Plugins => "plugin",
                Page::Keyboard => "keyboard",
                Page::Plugin(_) => "",
            };
            if matches!(page, Page::Plugin(_)) && !plugins_caption {
                plugins_caption = true;
                list = list.child(
                    div()
                        .px_2()
                        .pt_3()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child("Plugin settings"),
                );
            }
            let selected = !searching && page == self.page;
            let modified = self
                .rows
                .iter()
                .filter(|row| row.page() == page && row.modified())
                .count();
            let title = self.page_title(&page);
            let target = page.clone();
            let hover = theme.hover;
            list = list.child(
                div()
                    .id(SharedString::from(format!("settings-page-{title}")))
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if selected && nav_focused {
                        theme.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .when(selected, |d| {
                        d.bg(theme.selection_bg).text_color(theme.selection_text)
                    })
                    .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                    .when(!icon.is_empty(), |d| {
                        d.child(controls::glyph(
                            icon,
                            if selected {
                                theme.selection_text
                            } else {
                                theme.text_muted
                            },
                            15.,
                        ))
                    })
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .when(modified > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(if selected {
                                    theme.selection_text
                                } else {
                                    theme.accent
                                })
                                .child(modified.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.nav);
                        this.go(target.clone(), cx);
                    })),
            );
        }
        div()
            .w(px(216.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .rounded_l_lg()
            .border_r_1()
            .border_color(theme.border)
            .child(div().p_2().pb_0().child(controls::field(
                &self.search,
                None,
                search_focused,
                false,
                Some("search"),
                theme,
            )))
            .child(list)
            .into_any_element()
    }

    // ------------------------------------------------------------ rows

    fn card_row(theme: &Theme, edge: Edge) -> gpui::Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_4()
            .py_3()
            .bg(theme.surface)
            .border_color(theme.border)
            .border_l_1()
            .border_r_1()
            .border_b_1()
            .when(edge.first, |d| d.border_t_1().rounded_t_lg())
            .when(edge.last, gpui::Styled::rounded_b_lg)
    }

    fn caption(items: &mut Vec<AnyElement>, text: impl Into<SharedString>, theme: &Theme) {
        items.push(
            div()
                .flex_none()
                .pt_5()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child(text.into())
                .into_any_element(),
        );
    }

    fn note(items: &mut Vec<AnyElement>, text: impl Into<SharedString>, theme: &Theme) {
        items.push(
            div()
                .flex_none()
                .py_6()
                .text_color(theme.text_muted)
                .child(text.into())
                .into_any_element(),
        );
    }

    fn texts(
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        theme: &Theme,
    ) -> gpui::Div {
        let description: SharedString = description.into();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().child(title.into())),
            )
            .when(!description.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(description),
                )
            })
    }

    #[allow(clippy::too_many_lines)] // one arm per control kind
    fn control(
        &mut self,
        row: &Snapshot,
        item: usize,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.spec.key.clone();
        let invalid = self.errors.contains_key(&key);
        match row.spec.kind {
            SettingKind::Toggle => {
                let on = row.value == "true";
                let focus = self.handle(&format!("control:{key}"), cx);
                self.stop(&focus, Some(item), Some(&key));
                controls::switch(format!("switch-{key}"), on, &focus, theme)
                    .key_context("SettingControl")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.change(&key, (!on).to_string(), cx);
                    }))
                    .into_any_element()
            }
            SettingKind::Choice if row.spec.constraints.choices.len() <= SEGMENTED_MAX => {
                let focus = self.handle(&format!("control:{key}"), cx);
                self.stop(&focus, Some(item), Some(&key));
                let choices = row.spec.constraints.choices.clone();
                let current = choices.iter().position(|choice| *choice == row.value);
                let cycle_key = key.clone();
                let cycle = choices.clone();
                let mut frame = controls::segmented(format!("choice-{key}"), &focus, theme)
                    .key_context("SettingControl")
                    // Space and Enter reach the focused frame as a keyboard
                    // click; mouse clicks belong to the segments.
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if matches!(event, ClickEvent::Keyboard(_)) && !cycle.is_empty() {
                            let next = current.map_or(0, |index| (index + 1) % cycle.len());
                            this.change(&cycle_key, cycle[next].clone(), cx);
                        }
                    }));
                for (index, choice) in choices.into_iter().enumerate() {
                    let label = row.spec.choice_label(&choice).to_owned();
                    let key = key.clone();
                    let focus = focus.clone();
                    frame = frame.child(
                        controls::segment(
                            format!("choice-{key}-{choice}"),
                            label,
                            current == Some(index),
                            theme,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                window.focus(&focus);
                                this.change(&key, choice.clone(), cx);
                            },
                        )),
                    );
                }
                frame.into_any_element()
            }
            SettingKind::Choice => {
                let focus = self.handle(&format!("control:{key}"), cx);
                self.stop(&focus, Some(item), Some(&key));
                let open = self.dropdown.as_deref() == Some(key.as_str());
                let toggle = key.clone();
                let mut element = div().child(
                    controls::dropdown(
                        format!("choice-{key}"),
                        row.spec.choice_label(&row.value).to_owned(),
                        &focus,
                        theme,
                    )
                    .key_context("SettingControl")
                    .child(row.anchor.marker())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dropdown = if open { None } else { Some(toggle.clone()) };
                        cx.notify();
                    })),
                );
                if open {
                    let hover = theme.hover;
                    let mut panel = div()
                        .id(SharedString::from(format!("choices-{key}")))
                        .min_w(px(150.))
                        .p_1()
                        .flex()
                        .flex_col()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bg)
                        .text_color(theme.text)
                        .text_xs()
                        .font_family(self.font.clone())
                        .shadow_lg()
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.dropdown = None;
                            cx.notify();
                        }));
                    for choice in row.spec.constraints.choices.clone() {
                        let selected = choice == row.value;
                        let label = row.spec.choice_label(&choice).to_owned();
                        let key = key.clone();
                        panel = panel.child(
                            div()
                                .id(SharedString::from(format!("choice-{key}-{choice}")))
                                .h(px(26.))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .child(div().w(px(14.)).when(selected, |d| {
                                    d.child(controls::glyph("check", theme.accent, 14.))
                                }))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.dropdown = None;
                                    this.change(&key, choice.clone(), cx);
                                })),
                        );
                    }
                    element = element.child(row.anchor.popup(panel));
                }
                element.into_any_element()
            }
            SettingKind::Data => div().into_any_element(),
            kind => {
                let Some(input) = row.input.clone() else {
                    return div().into_any_element();
                };
                let focus = input.focus_handle(cx);
                let focused = focus.is_focused(window);
                let numeric = kind == SettingKind::Integer;
                self.stop(&focus, Some(item), numeric.then_some(key.as_str()));
                let width = match kind {
                    SettingKind::Integer => 64.,
                    SettingKind::Number => 90.,
                    _ => TEXT_WIDTH,
                };
                let mut group = div().flex().flex_none().items_center().gap(px(4.));
                if numeric {
                    let (less, more) = (key.clone(), key.clone());
                    let (less_focus, more_focus) = (
                        self.handle(&format!("less:{key}"), cx),
                        self.handle(&format!("more:{key}"), cx),
                    );
                    group = group
                        .key_context("SettingNumber")
                        .child(
                            controls::icon_button(
                                format!("less-{key}"),
                                "minus",
                                &less_focus,
                                theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.adjust_key(&less, -1, cx);
                                },
                            )),
                        )
                        .child(controls::field(
                            &input,
                            Some(width),
                            focused,
                            invalid,
                            None,
                            theme,
                        ))
                        .child(
                            controls::icon_button(
                                format!("more-{key}"),
                                "plus",
                                &more_focus,
                                theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.adjust_key(&more, 1, cx);
                                },
                            )),
                        );
                } else {
                    group = group.child(controls::field(
                        &input,
                        Some(width),
                        focused,
                        invalid,
                        None,
                        theme,
                    ));
                }
                if !row.spec.unit.is_empty() {
                    group = group.child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(row.spec.unit.clone()),
                    );
                }
                if kind == SettingKind::Path {
                    let browse = self.handle(&format!("browse:{key}"), cx);
                    self.stop(&browse, Some(item), None);
                    let key = key.clone();
                    group = group.child(
                        controls::button(
                            format!("browse-{key}"),
                            "Browse…",
                            None,
                            ButtonStyle::Plain,
                            &browse,
                            theme,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.browse(&key, cx))),
                    );
                }
                group.into_any_element()
            }
        }
    }

    fn setting_row(
        &mut self,
        row: &Snapshot,
        edge: Edge,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let item = items.len();
        let key = row.spec.key.clone();
        self.row_items.insert(key.clone(), item);
        let error = self.errors.get(&key).cloned();
        let mut texts = Self::texts(row.spec.title.clone(), row.spec.description.clone(), theme);
        if row.spec.restart_required {
            texts = texts.child(div().flex().child(controls::badge(
                "Applies after restart",
                theme.text_muted,
                theme,
            )));
        }
        if let Some(error) = error {
            texts = texts.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(theme.error)
                    .child(controls::glyph("warning", theme.error, 13.))
                    .child(error),
            );
        }
        // Left of the control, so controls stay aligned on the right edge
        // whether or not a row is modified.
        let revert = row.modified.then(|| {
            let focus = self.handle(&format!("revert:{key}"), cx);
            self.stop(&focus, Some(item), None);
            let font = self.font.clone();
            let tip: SharedString = if row.default_text.is_empty() {
                "Restore the default (not set)".into()
            } else {
                format!(
                    "Restore the default: {}",
                    row.spec.choice_label(&row.default_text)
                )
                .into()
            };
            let key = key.clone();
            controls::icon_button(format!("revert-{key}"), "revert", &focus, theme)
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
                .on_click(cx.listener(move |this, _, _, cx| this.reset(&key, cx)))
                .into_any_element()
        });
        // Built after the revert button: traversal follows the visual order.
        let control = self.control(row, item, theme, window, cx);
        let highlighted = self.highlight.as_deref() == Some(key.as_str());
        items.push(
            Self::card_row(theme, edge)
                .when(highlighted, |d| d.bg(theme.highlight))
                .child(texts)
                .children(revert)
                .child(control)
                .into_any_element(),
        );
    }

    /// The editable settings of one owner, grouped by section.
    fn owner_rows(
        &mut self,
        owner: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let rows: Vec<Snapshot> = self
            .rows
            .iter()
            .filter(|row| row.spec.owner() == owner)
            .map(Snapshot::of)
            .collect();
        let mut start = 0;
        while start < rows.len() {
            let section = rows[start].spec.section.clone();
            let end = rows[start..]
                .iter()
                .position(|row| row.spec.section != section)
                .map_or(rows.len(), |offset| start + offset);
            if section.is_empty() {
                items.push(
                    div()
                        .flex_none()
                        .h(px(if start == 0 { 0. } else { 16. }))
                        .into_any_element(),
                );
            } else {
                Self::caption(items, section, theme);
            }
            let group = &rows[start..end];
            for (index, row) in group.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == group.len(),
                };
                self.setting_row(row, edge, items, theme, window, cx);
            }
            start = end;
        }
        rows.len()
    }

    fn plugin_row(
        &mut self,
        plugin: &PluginInfo,
        link: bool,
        edge: Edge,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) {
        let item = items.len();
        let id = plugin.id;
        let pending = self.toggling.contains_key(id);
        let shown = self.toggling.get(id).copied().unwrap_or(plugin.enabled);
        let title = if link {
            plugin.title.clone()
        } else {
            "Enabled".to_owned()
        };
        let mut texts = Self::texts(title, plugin.description.clone(), theme);
        if let Some(requirements) = &self.requirements {
            for status in requirements.iter().filter(|status| status.plugin == id) {
                let (icon, color, text) = if status.found {
                    ("check", theme.text_muted, format!("{} found", status.name))
                } else {
                    (
                        "warning",
                        match (plugin.enabled, status.required) {
                            (true, true) => theme.error,
                            (true, false) => theme.warning,
                            (false, _) => theme.text_muted,
                        },
                        format!("{} not found — {}", status.name, status.purpose),
                    )
                };
                texts = texts.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_xs()
                        .text_color(color)
                        .child(controls::glyph(icon, color, 13.))
                        .child(text),
                );
            }
        }
        let mut row = Self::card_row(theme, edge).child(texts);
        if link && plugin.enabled && self.rows.iter().any(|row| row.spec.owner() == id) {
            let focus = self.handle(&format!("plugin-settings:{id}"), cx);
            self.stop(&focus, Some(item), None);
            row = row.child(
                controls::button(
                    format!("plugin-settings-{id}"),
                    "Settings",
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.nav);
                    this.go(Page::Plugin(id.to_owned()), cx);
                })),
            );
        }
        let focus = self.handle(&format!("plugin:{id}"), cx);
        self.stop(&focus, Some(item), None);
        row = row.child(
            controls::switch(format!("plugin-{id}"), shown, &focus, theme)
                .when(pending, |d| d.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.toggling.contains_key(id) {
                        this.set_plugin(id, !shown, cx);
                    }
                })),
        );
        items.push(row.into_any_element());
    }

    fn binding_row(&self, hint: &KeyHint, edge: Edge, theme: &Theme) -> AnyElement {
        let mut keys = div().flex().flex_none().items_center().gap(px(10.));
        for stroke in crate::keymap::key_caps(&hint.keys) {
            let mut caps = div().flex().items_center().gap(px(3.));
            for cap in stroke {
                caps = caps.child(controls::keycap(cap, theme));
            }
            keys = keys.child(caps);
        }
        Self::card_row(theme, edge)
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.command_title(hint)),
            )
            .child(keys)
            .into_any_element()
    }

    // ------------------------------------------------------------ pages

    fn page_general(
        &mut self,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.owner_rows(CORE_OWNER, items, theme, window, cx);
        Self::caption(items, "Settings file", theme);
        let item = items.len();
        let location = self.app.settings.path().map_or_else(
            || "Settings are kept in memory for this session.".to_owned(),
            |path| {
                let shown = std::env::var_os("HOME")
                    .and_then(|home| {
                        path.strip_prefix(home)
                            .ok()
                            .map(|rest| format!("~/{}", rest.display()))
                    })
                    .unwrap_or_else(|| path.display().to_string());
                format!("{shown}. It lists only what differs from the defaults.")
            },
        );
        let open = self.handle("open-file", cx);
        self.stop(&open, Some(item), None);
        items.push(
            Self::card_row(
                theme,
                Edge {
                    first: true,
                    last: false,
                },
            )
            .child(Self::texts("Edit as text", location, theme))
            .child(
                controls::button(
                    "settings-open-file",
                    "Open",
                    Some("external"),
                    ButtonStyle::Plain,
                    &open,
                    theme,
                )
                .on_click(cx.listener(|this, _, _, _| this.open_file())),
            )
            .into_any_element(),
        );
        let item = items.len();
        let modified = self.rows.iter().filter(|row| row.modified()).count();
        let reset = self.handle("reset-all", cx);
        let description = match modified {
            0 => "Every setting already has its default value.".to_owned(),
            1 => {
                "1 setting differs from its default. Plugins stay enabled or disabled as they are."
                    .to_owned()
            }
            n => format!(
                "{n} settings differ from their defaults. Plugins stay enabled or disabled as they are."
            ),
        };
        let mut row = Self::card_row(
            theme,
            Edge {
                first: false,
                last: true,
            },
        )
        .child(Self::texts("Restore defaults", description, theme));
        if modified > 0 {
            self.stop(&reset, Some(item), None);
            row = row.child(
                controls::button(
                    "settings-reset-all",
                    if self.reset_armed {
                        "Click again to restore"
                    } else {
                        "Restore all…"
                    },
                    None,
                    ButtonStyle::Danger,
                    &reset,
                    theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.reset_all(cx))),
            );
        }
        items.push(row.into_any_element());
    }

    fn page_plugin(
        &mut self,
        id: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.id == id).cloned() else {
            Self::note(items, "This plugin is not part of this build.", theme);
            return;
        };
        self.plugin_row(
            &plugin,
            false,
            Edge {
                first: true,
                last: true,
            },
            items,
            theme,
            cx,
        );
        if !plugin.enabled {
            Self::note(items, "Turn the plugin on to change its settings.", theme);
            return;
        }
        items.push(div().flex_none().h(px(16.)).into_any_element());
        self.owner_rows(id, items, theme, window, cx);
    }

    fn page_keyboard(&self, query: &str, items: &mut Vec<AnyElement>, theme: &Theme) -> usize {
        let mut hints: Vec<(String, &KeyHint)> = self
            .key_hints
            .iter()
            .map(|hint| (crate::keymap::context_label(&hint.context), hint))
            .filter(|(context, hint)| {
                query.is_empty()
                    || matches(query, &[context, &hint.keys, &self.command_title(hint)])
            })
            .collect();
        // "Anywhere" first, then places alphabetically; stable within a place.
        hints.sort_by_key(|(context, _)| (context != "Anywhere", context.clone()));
        let mut start = 0;
        while start < hints.len() {
            let end = hints[start..]
                .iter()
                .position(|(context, _)| *context != hints[start].0)
                .map_or(hints.len(), |offset| start + offset);
            Self::caption(items, hints[start].0.clone(), theme);
            let group = &hints[start..end];
            for (index, (_, hint)) in group.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == group.len(),
                };
                items.push(self.binding_row(hint, edge, theme));
            }
            start = end;
        }
        hints.len()
    }

    fn search_results(
        &mut self,
        query: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let mut found = 0;
        let mut owners: Vec<String> = self
            .rows
            .iter()
            .map(|row| row.spec.owner().to_owned())
            .collect();
        owners.dedup();
        for owner in owners {
            let title = self.page_title(&Page::of_owner(&owner));
            let rows: Vec<Snapshot> = self
                .rows
                .iter()
                .filter(|row| row.spec.owner() == owner && spec_matches(query, &row.spec, &title))
                .map(Snapshot::of)
                .collect();
            if rows.is_empty() {
                continue;
            }
            found += rows.len();
            Self::caption(items, title, theme);
            for (index, row) in rows.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == rows.len(),
                };
                self.setting_row(row, edge, items, theme, window, cx);
            }
        }
        let plugins: Vec<PluginInfo> = self
            .plugins
            .iter()
            .filter(|plugin| matches(query, &[&plugin.title, &plugin.description, plugin.id]))
            .cloned()
            .collect();
        if !plugins.is_empty() {
            found += plugins.len();
            Self::caption(items, "Plugins", theme);
            for (index, plugin) in plugins.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == plugins.len(),
                };
                self.plugin_row(plugin, true, edge, items, theme, cx);
            }
        }
        found += self.page_keyboard(query, items, theme);
        if found == 0 {
            Self::note(items, format!("Nothing matches “{query}”."), theme);
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        self.stops.clear();
        self.row_items.clear();
        let search_focus = self.search.focus_handle(cx);
        self.stop(&search_focus, None, None);
        let nav = self.nav.clone();
        self.stop(&nav, None, None);
        let sidebar = self.render_sidebar(&theme, window, cx);

        let query = self.query(cx);
        let mut items: Vec<AnyElement> = Vec::new();
        let (title, subtitle): (String, String) = if query.is_empty() {
            let page = self.page.clone();
            let subtitle = match &page {
                Page::General => "How bibli handles files. Changes apply as you make them.",
                Page::Appearance => "How bibli looks.",
                Page::Plugins => {
                    "Everything beyond the core is a plugin. Turning one off removes its commands, views and settings until it is turned on again."
                }
                Page::Keyboard => "Every shortcut, grouped by where it applies.",
                Page::Plugin(_) => "",
            };
            match &page {
                Page::General => self.page_general(&mut items, &theme, window, cx),
                Page::Appearance => {
                    if self.owner_rows(UI_OWNER, &mut items, &theme, window, cx) == 0 {
                        Self::note(&mut items, "Nothing to adjust here yet.", &theme);
                    }
                }
                Page::Plugins => {
                    let mut plugins = self.plugins.clone();
                    plugins.sort_by_key(|plugin| plugin.title.to_lowercase());
                    for (index, plugin) in plugins.iter().enumerate() {
                        let edge = Edge {
                            first: index == 0,
                            last: index + 1 == plugins.len(),
                        };
                        self.plugin_row(plugin, true, edge, &mut items, &theme, cx);
                    }
                }
                Page::Keyboard => {
                    self.page_keyboard("", &mut items, &theme);
                }
                Page::Plugin(id) => self.page_plugin(id, &mut items, &theme, window, cx),
            }
            (self.page_title(&page), subtitle.to_owned())
        } else {
            self.search_results(&query, &mut items, &theme, window, cx);
            ("Search results".to_owned(), String::new())
        };

        if let Some(key) = self.take_scroll_target()
            && let Some(item) = self.row_items.get(&key)
        {
            self.scroll.scroll_to_item(*item);
        }

        let close = self.handle("close", cx);
        self.stop(&close, None, None);
        let status = if self.saving() {
            Some((None, "Saving…"))
        } else if self.saved_flash {
            Some((Some("check"), "Saved"))
        } else {
            None
        };
        let header = div()
            .flex_none()
            .px_6()
            .pt_5()
            .pb_3()
            .flex()
            .items_start()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child(title))
                    .when(!subtitle.is_empty(), |d| {
                        d.child(div().text_xs().text_color(theme.text_muted).child(subtitle))
                    }),
            )
            .children(status.map(|(icon, text)| {
                div()
                    .h(px(controls::HEIGHT))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(5.))
                    .text_xs()
                    .text_color(theme.text_muted)
                    .children(icon.map(|name| controls::glyph(name, theme.text_muted, 13.)))
                    .child(text)
            }))
            .child(
                controls::icon_button("settings-close", "close", &close, &theme)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
            );
        let body = div()
            .id("settings-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .px_6()
            .pb_6()
            .children(items);
        let panel = div()
            .w(relative(0.94))
            .max_w(px(920.))
            .h(relative(0.9))
            .max_h(px(680.))
            .flex()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg)
            .shadow_lg()
            // Whole-pixel line heights keep every row height integral, so the
            // one-pixel dividers between rows never fall between two pixels.
            .line_height(px(20.))
            .track_focus(&self.root)
            .key_context("Settings")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header)
                    // A column, like the one the body used to sit in directly:
                    // the page keeps the height it scrolls within.
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(body)
                            .child(crate::elements::scrollbar::vertical(
                                "settings-scrollbar",
                                crate::elements::scrollbar::Scrolls::Offset(self.scroll.clone()),
                            )),
                    ),
            );
        // The backdrop closes the dialog: nothing is lost, every change is
        // already applied.
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .bg(theme.scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)),
            )
            .child(panel)
    }
}
