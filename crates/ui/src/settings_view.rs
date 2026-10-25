//! Rendering of the Settings dialog (state and behaviour: `settings.rs`).
//!
//! The page is a flat list of children inside one scroll container, so a
//! keyboard stop can be brought into view by its child index. Cards are drawn
//! by giving the first and last row of a group their rounded corners.

use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, MouseButton, SharedString, Window, div,
    prelude::*, px, relative,
};
use skwir_core::plugin::{Category, PluginInfo};
use skwir_core::settings::{SettingKind, SettingSpec};

use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::input::TextInput;
use crate::settings::{Block, Page, Row, SettingsEvent, SettingsView, Stop};
use crate::theme::Theme;

/// Segmented controls hold at most this many choices, and only while their
/// labels fit beside the row's texts (`elements::adaptive::Fit`); otherwise
/// a dropdown.
const SEGMENTED_MAX: usize = 4;
const TEXT_WIDTH: f32 = 260.;
/// A text field narrows to this before the row's texts give way.
const FIELD_MIN: f32 = 160.;
/// A row's title and description keep this much width whatever the
/// control beside them: the control shrinks, or picks a narrower form.
const TEXTS_MIN: f32 = 240.;

/// How wide a choice is on a table's line, where it is a dropdown: its
/// column's dropdowns line up.
const TABLE_CHOICE: f32 = 210.;

/// A dropdown of more choices than this opens with a field that narrows
/// them (`settings::Narrowed`): the icon themes, bat's themes.
pub(crate) const FILTERED: usize = 12;
/// The rows a long dropdown shows at once; the rest scroll.
const LONG_ROWS: usize = 12;
const CHOICE_ROW: f32 = 26.;
/// What a font's specimen writes (ADR-073): letters and figures, and for
/// code the shapes that tell characters apart.
const SPECIMEN: &str = "Sphinx of black quartz, judge my vow. 0123456789";
const SPECIMEN_CODE: &str = "if (x != 0O) { l1 = I[i]; } // => <=";

/// Whether `spec`'s dropdown opens as a list that narrows
/// (`settings::Narrowed`): one of more than `FILTERED` choices, and a
/// font's however few the system has, for the specimen its list writes
/// (ADR-073): the code font is offered only the monospaced families, often
/// fewer.
pub(crate) fn narrows(spec: &SettingSpec) -> bool {
    spec.constraints.choices.len() > FILTERED
        || crate::settings::FONT_KEYS.contains(&spec.key.as_str())
}

/// The family a font setting's control and specimen are written in when
/// it says Automatic: what its role resolved to (`Theme::font`…).
fn font_role(key: &str, theme: &Theme) -> Option<SharedString> {
    use crate::settings::{CODE_FONT_KEY, FONT_KEY, READING_FONT_KEY};
    match key {
        FONT_KEY => Some(theme.font.clone()),
        READING_FONT_KEY => Some(theme.reading_font.clone()),
        CODE_FONT_KEY => Some(theme.code_font.clone()),
        _ => None,
    }
}

/// The width of a table's column of a kind of control, its way back
/// included.
fn column_width(kind: SettingKind) -> f32 {
    controls::HEIGHT
        + 8.
        + match kind {
            SettingKind::Toggle => 44.,
            SettingKind::Integer | SettingKind::Number => 140.,
            _ => TABLE_CHOICE,
        }
}

/// Where a row sits in its card.
#[derive(Clone, Copy)]
pub(crate) struct Edge {
    pub(crate) first: bool,
    pub(crate) last: bool,
}

/// A row of a card: a setting, a record, a plugin.
pub(crate) fn card_row(theme: &Theme, edge: Edge) -> gpui::Div {
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

/// The place of a row's revert button, held whether or not the row is
/// modified: changing a value or restoring it never moves the texts or the
/// control beside it.
pub(crate) fn revert_slot(size: f32, button: Option<AnyElement>) -> gpui::Div {
    div().flex_none().size(px(size)).children(button)
}

/// What a setting row needs, detached from `self` so controls can borrow it.
#[derive(Clone)]
struct Snapshot {
    spec: SettingSpec,
    /// What its control is named: its title, after its section's on a
    /// table's line.
    label: SharedString,
    value: String,
    input: Option<Entity<TextInput>>,
    anchor: PopoverAnchor,
    modified: bool,
    default_text: String,
    /// It can be changed: it is under no switch that is off
    /// (`SettingSpec::under`).
    available: bool,
}

impl Snapshot {
    fn of(row: &Row, view: &SettingsView) -> Self {
        Snapshot {
            label: row.spec.title.clone().into(),
            spec: row.spec.clone(),
            value: row.value.clone(),
            input: row.input.clone(),
            anchor: row.anchor.clone(),
            modified: row.modified(),
            default_text: row.default_text(),
            available: view.available(&row.spec),
        }
    }
}

fn matches(query: &str, haystack: &[&str]) -> bool {
    haystack
        .iter()
        .any(|text| text.to_lowercase().contains(query))
}

/// A setting found by its words, its page's title or its plugin's.
fn spec_matches(query: &str, spec: &SettingSpec, titles: [&str; 2]) -> bool {
    matches(
        query,
        &[
            &spec.title,
            &spec.description,
            &spec.key,
            &spec.section,
            titles[0],
            titles[1],
        ],
    ) || spec
        .constraints
        .choice_labels
        .iter()
        .any(|label| label.to_lowercase().contains(query))
}

impl SettingsView {
    pub(crate) fn stop(
        &mut self,
        focus: &gpui::FocusHandle,
        item: Option<usize>,
        key: Option<&str>,
    ) {
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
        // Its pages, tabs of the dialog: the page shown is the one the
        // keys move, said as focused while the list has the keyboard.
        let mut list = div()
            .id("settings-nav")
            .role(gpui::Role::TabList)
            .aria_label("Pages")
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
            let icon = page.builtin().map_or("", |b| b.icon);
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
            let modified = if page == Page::Changed {
                self.changed_count()
            } else {
                self.rows
                    .iter()
                    .filter(|row| row.page() == page && row.modified())
                    .count()
            };
            let title = self.page_title(&page);
            let target = page.clone();
            let hover = theme.hover;
            list = list.child(
                div()
                    .id(SharedString::from(format!("settings-page-{title}")))
                    .role(gpui::Role::Tab)
                    .when(self.a11y, |d| {
                        d.aria_label(title.clone()).aria_selected(selected)
                    })
                    .when(selected, StatefulInteractiveElement::aria_active_descendant)
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
                    .child(crate::elements::clip::clipped(
                        div().id("page-title").flex_1().min_w_0().truncate(),
                        title,
                    ))
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
                        window.focus(&this.nav, cx);
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
            .min_w(px(TEXTS_MIN))
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // A long title (a store of notes on a table's line)
                    // wraps rather than runs under the controls.
                    .child(div().min_w_0().child(title.into())),
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

    /// A setting's control as assistive technology hears it: named by the
    /// setting's title, described by its description, as the texts beside
    /// it say them.
    fn named<E: StatefulInteractiveElement>(&self, el: E, row: &Snapshot) -> E {
        if !self.a11y {
            return el;
        }
        let el = el
            .aria_label(row.label.clone())
            .aria_disabled(!row.available);
        if row.spec.description.is_empty() {
            el
        } else {
            el.aria_description(row.spec.description.clone())
        }
    }

    /// A setting's control; a choice on a table's line is always a
    /// dropdown, which keeps its column's width.
    #[allow(clippy::too_many_lines)] // one arm per control kind
    fn control(
        &mut self,
        row: &Snapshot,
        item: usize,
        in_table: bool,
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
                self.named(
                    controls::switch(format!("switch-{key}"), on, &focus, theme),
                    row,
                )
                .key_context("SettingControl")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change(&key, (!on).to_string(), cx);
                }))
                .into_any_element()
            }
            SettingKind::Choice => {
                let focus = self.handle(&format!("control:{key}"), cx);
                self.stop(&focus, Some(item), Some(&key));
                let dropdown = self.choice_dropdown(row, in_table, &focus, theme, cx);
                if in_table || row.spec.constraints.choices.len() > SEGMENTED_MAX {
                    return dropdown;
                }
                // Buttons where their labels fit beside the texts, else the
                // dropdown: measured, never counted in characters.
                crate::elements::adaptive::Fit {
                    variants: vec![self.choice_buttons(row, &focus, theme, cx), dropdown],
                    cuts: false,
                }
                .into_any_element()
            }
            SettingKind::Records => {
                let focus = self.handle(&format!("control:{key}"), cx);
                self.stop(&focus, Some(item), Some(&key));
                let editing = self.drafts.contains_key(&key);
                // Kept by their owner elsewhere: its command adds one,
                // asking what it needs itself.
                let add = row
                    .spec
                    .constraints
                    .record_source
                    .as_ref()
                    .map(|source| source.add.clone());
                controls::button(
                    format!("add-{key}"),
                    format!("Add {}", row.spec.constraints.record_name),
                    Some("plus"),
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .key_context("SettingControl")
                .when(editing, |b| b.opacity(0.5))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(add) = &add {
                        let invocation = skwir_core::command::Invocation::new(add.as_str());
                        if let Err(error) = this.app.dispatch(&invocation) {
                            this.app
                                .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}"));
                        }
                    } else if !this.drafts.contains_key(&key) {
                        this.edit_record(&key, None, window, cx);
                    }
                }))
                .into_any_element()
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
                // A number keeps its width; a text field narrows to
                // `FIELD_MIN` before the texts beside it give way.
                let mut group = div().flex().items_center().gap(px(4.)).map(|d| {
                    if numeric || kind == SettingKind::Number {
                        d.flex_none()
                    } else {
                        d.w(px(width)).min_w(px(FIELD_MIN)).flex_shrink_1()
                    }
                });
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
                                "Less",
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
                                "More",
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
                    let fixed = (kind == SettingKind::Number).then_some(width);
                    group = group.child(controls::field(
                        &input, fixed, focused, invalid, None, theme,
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
                        // Which setting it browses for, as the row says.
                        .when(self.a11y, |d| d.aria_description(row.spec.title.clone()))
                        .on_click(cx.listener(move |this, _, _, cx| this.browse(&key, cx))),
                    );
                }
                group.into_any_element()
            }
        }
    }

    /// A choice as segmented buttons, one per value.
    fn choice_buttons(
        &self,
        row: &Snapshot,
        focus: &gpui::FocusHandle,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.spec.key.clone();
        let choices = row.spec.constraints.choices.clone();
        let current = choices.iter().position(|choice| *choice == row.value);
        let cycle_key = key.clone();
        let cycle = choices.clone();
        let mut frame = self
            .named(
                controls::segmented(format!("choice-{key}"), focus, theme),
                row,
            )
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.change(&key, choice.clone(), cx);
                })),
            );
        }
        frame.into_any_element()
    }

    /// A choice as a dropdown of its values.
    /// A choice as a dropdown of its values; on a table's line, as wide as
    /// its column, so the column's dropdowns line up.
    fn choice_dropdown(
        &self,
        row: &Snapshot,
        in_table: bool,
        focus: &gpui::FocusHandle,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.spec.key.clone();
        let open = self.dropdown.as_deref() == Some(key.as_str());
        let long = narrows(&row.spec);
        let toggle = key.clone();
        // A font's control is written in the font it gives.
        let written_in = font_role(&key, theme);
        let mut element = div().child(
            self.named(
                controls::dropdown(
                    format!("choice-{key}"),
                    row.spec.choice_label(&row.value).to_owned(),
                    focus,
                    theme,
                )
                .when(in_table, |d| d.min_w(px(TABLE_CHOICE)))
                .when_some(written_in, gpui::Styled::font_family),
                row,
            )
            .aria_expanded(open)
            .key_context("SettingControl")
            .child(row.anchor.marker())
            .on_click(cx.listener(move |this, _, window, cx| {
                if open && long {
                    this.close_choices(window, cx);
                } else if long {
                    this.open_choices(&toggle, window, cx);
                } else {
                    this.dropdown = if open { None } else { Some(toggle.clone()) };
                    cx.notify();
                }
            })),
        );
        if open && long {
            if let Some(panel) = self.long_choices(row, theme, cx) {
                element = element.child(row.anchor.popup(panel));
            }
        } else if open {
            let hover = theme.hover;
            // Its choices scroll in the height the popover leaves them
            // under the row; a long list narrows instead (`long_choices`).
            let mut panel = div()
                .id(SharedString::from(format!("choices-{key}")))
                .min_w(px(150.))
                .min_h_0()
                .overflow_y_scroll()
                .p_1()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .text_color(theme.text)
                .text_xs()
                .font_family(theme.font.clone())
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

    /// A long dropdown's panel: its field, the choices it keeps a row at a
    /// time as they scroll into sight, and for a font the one highlighted
    /// written in itself. Only that one is: a family drawn is loaded and
    /// kept by the text system.
    fn long_choices(
        &self,
        row: &Snapshot,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let narrowed = self.narrowed.as_ref().filter(|n| n.key == row.spec.key)?;
        let key = row.spec.key.clone();
        let count = narrowed.kept.len();
        let field = narrowed.field.focus_handle(cx);
        let (value, a11y) = (row.value.clone(), self.a11y);
        let rows = gpui::uniform_list(
            SharedString::from(format!("choices-{key}-rows")),
            count,
            cx.processor(
                move |this: &mut SettingsView, range: std::ops::Range<usize>, _, cx| {
                    let Some(narrowed) = &this.narrowed else {
                        return Vec::new();
                    };
                    let theme = cx.global::<Theme>().clone();
                    range
                        .filter_map(|at| {
                            let kept = *narrowed.kept.get(at)?;
                            let (choice, label) = &narrowed.choices[kept];
                            let highlighted = at == narrowed.highlighted;
                            let named = a11y.then(|| SharedString::from(label.clone()));
                            let option = div().id(crate::elements::known_by("choice", kept));
                            Some(
                                crate::modal::option(option, named, at, highlighted, &field)
                                    .w_full()
                                    .h(px(CHOICE_ROW))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .when(highlighted, |d| {
                                        d.bg(theme.selection_bg).text_color(theme.selection_text)
                                    })
                                    .child(div().w(px(14.)).flex_none().when(
                                        *choice == value,
                                        |d| {
                                            let tick = if highlighted {
                                                theme.selection_text
                                            } else {
                                                theme.accent
                                            };
                                            d.child(controls::glyph("check", tick, 14.))
                                        },
                                    ))
                                    .child(div().min_w_0().truncate().child(label.clone()))
                                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                        if let Some(narrowed) = &mut this.narrowed
                                            && narrowed.highlighted != at
                                        {
                                            narrowed.highlighted = at;
                                            cx.notify();
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(narrowed) = &mut this.narrowed {
                                            narrowed.highlighted = at;
                                        }
                                        this.take_choice(window, cx);
                                    }))
                                    .into_any_element(),
                            )
                        })
                        .collect()
                },
            ),
        )
        .track_scroll(&narrowed.scroll)
        .h_full();
        // Up to `LONG_ROWS` rows, fewer where the popover leaves less
        // under the row (`popover::hang`): the list gives way, never the
        // field nor the specimen, which would run past the window's foot.
        let list = crate::modal::options("choices-list", "Choices", count, a11y)
            .relative()
            .h(px(CHOICE_ROW * count.clamp(1, LONG_ROWS) as f32))
            .min_h_0()
            .child(rows)
            .when(count == 0, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .px_2()
                        .h(px(CHOICE_ROW))
                        .flex()
                        .items_center()
                        .text_color(theme.text_muted)
                        .child("None holds what is typed"),
                )
            });
        let specimen = font_role(&key, theme).map(|role| {
            let family: SharedString = match narrowed.highlighted_value() {
                Some(family) if family != crate::settings::FONT_AUTO => family.to_owned().into(),
                _ => role,
            };
            let text = if key == crate::settings::CODE_FONT_KEY {
                SPECIMEN_CODE
            } else {
                SPECIMEN
            };
            // Asked whether it loads before it is drawn: a family the
            // index lists may still fail to, and gpui would search a
            // fallback each frame.
            let loads = cx
                .text_system()
                .font_id(&gpui::font(family.clone()))
                .is_ok();
            div()
                .flex_none()
                .border_t_1()
                .border_color(theme.border)
                .mt_1()
                .px_2()
                .pt_2()
                .pb_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(theme.text_muted).child(family.clone()))
                .child(if loads {
                    div().font_family(family).text_base().child(text)
                } else {
                    div()
                        .text_color(theme.text_muted)
                        .child("This font cannot be drawn")
                })
        });
        Some(
            div()
                .id(SharedString::from(format!("choices-{key}")))
                .key_context("SettingChoices")
                .w(px(300.))
                .min_h_0()
                .p_1()
                .flex()
                .flex_col()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .text_color(theme.text)
                .text_xs()
                .font_family(theme.font.clone())
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    this.close_choices(window, cx);
                }))
                // The field has the keyboard as long as the list is open.
                .child(div().flex_none().flex().child(controls::field(
                    &narrowed.field,
                    None,
                    true,
                    false,
                    Some("search"),
                    theme,
                )))
                .child(list)
                .children(specimen)
                .into_any_element(),
        )
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
        let mut texts = self
            .described(row.spec.title.clone(), &row.spec.description, &key, theme)
            // Beneath the switch it is under.
            .when(!row.spec.under.is_empty(), |d| d.pl(px(24.)));
        if row.spec.restart_required {
            texts = texts.child(div().flex().child(controls::badge(
                "Applies after restart",
                theme.text_muted,
                theme,
            )));
        }
        if let Some(error) = error {
            texts = texts.child(Self::error_line(error, theme));
        }
        // Left of the control, so controls stay aligned on the right edge,
        // in a slot every row keeps (`revert_slot`).
        let revert = self.revert_button(row, item, theme, cx);
        // Built after the revert button: traversal follows the visual order.
        let control = div()
            .flex_none()
            .when(!row.available, |d| d.opacity(0.5))
            .child(self.control(row, item, false, theme, window, cx));
        let highlighted = self.highlight.as_deref() == Some(key.as_str());
        // A list's records are rows of the same card, under its own.
        let records = row.spec.kind == SettingKind::Records;
        let below = records
            && (!crate::settings_records::records(&row.value).is_empty()
                || self.drafts.contains_key(&key));
        items.push(
            card_row(
                theme,
                Edge {
                    first: edge.first,
                    last: edge.last && !below,
                },
            )
            .when(highlighted, |d| d.bg(theme.highlight))
            .child(texts)
            .child(revert_slot(controls::HEIGHT, revert))
            .child(control)
            .into_any_element(),
        );
        if records {
            self.record_rows(&key, &row.value, edge.last, item, items, theme, window, cx);
        }
    }

    /// A setting's title and the first sentence of its description: the
    /// whole description is the sentence's tooltip, and what the control's
    /// description says to a screen reader (ADR-072).
    fn described(
        &self,
        title: impl Into<SharedString>,
        description: &str,
        key: &str,
        theme: &Theme,
    ) -> gpui::Div {
        let (shown, more) = crate::settings::first_sentence(description);
        let full: SharedString = description.to_owned().into();
        Self::texts(title, "", theme).when(!shown.is_empty(), |d| {
            d.child(
                div()
                    .id(SharedString::from(format!("description-{key}")))
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(shown.to_owned())
                    .when(more, |d| {
                        d.tooltip(move |_, cx| crate::toolbar::tooltip(full.clone(), cx))
                    }),
            )
        })
    }

    /// Why a change was refused, under the texts of its row.
    fn error_line(error: String, theme: &Theme) -> gpui::Div {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_xs()
            .text_color(theme.error)
            .child(controls::glyph("warning", theme.error, 13.))
            .child(div().flex_1().min_w_0().child(error))
    }

    /// The button restoring a setting's default, while it differs from it.
    fn revert_button(
        &mut self,
        row: &Snapshot,
        item: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !row.modified {
            return None;
        }
        let key = row.spec.key.clone();
        let focus = self.handle(&format!("revert:{key}"), cx);
        self.stop(&focus, Some(item), None);
        let tip: SharedString = if row.default_text.is_empty() {
            "Restore the default (not set)".into()
        } else {
            format!(
                "Restore the default: {}",
                row.spec.choice_label(&row.default_text)
            )
            .into()
        };
        Some(
            controls::icon_button(format!("revert-{key}"), "revert", "Revert", &focus, theme)
                .when(self.a11y, |d| d.aria_description(row.label.clone()))
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .on_click(cx.listener(move |this, _, _, cx| this.reset(&key, cx)))
                .into_any_element(),
        )
    }

    /// The header of a table: its columns' titles over their controls.
    fn table_header(columns: &[(String, SettingKind)], items: &mut Vec<AnyElement>, theme: &Theme) {
        let mut header = div()
            .flex_none()
            .pt_5()
            .pb_2()
            .px_4()
            .flex()
            .gap_4()
            .text_xs()
            .text_color(theme.text_muted)
            .child(div().flex_1().min_w(px(TEXTS_MIN)));
        for (title, kind) in columns {
            header = header.child(
                div()
                    .flex_none()
                    .w(px(column_width(*kind)))
                    .flex()
                    .justify_end()
                    .child(title.clone()),
            );
        }
        items.push(header.into_any_element());
    }

    /// A line of a table: its section's title and its first setting's
    /// description, then a cell a setting, each with its way back.
    #[allow(clippy::too_many_arguments)] // a row's, with its cells
    fn table_line(
        &mut self,
        title: &str,
        cells: &[Snapshot],
        edge: Edge,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let item = items.len();
        let Some(first) = cells.first() else { return };
        for cell in cells {
            self.row_items.insert(cell.spec.key.clone(), item);
        }
        let mut texts = self.described(
            title.to_owned(),
            &first.spec.description,
            &first.spec.key,
            theme,
        );
        for cell in cells {
            if let Some(error) = self.errors.get(&cell.spec.key).cloned() {
                texts = texts.child(Self::error_line(error, theme));
            }
        }
        let highlighted = cells
            .iter()
            .any(|cell| self.highlight.as_deref() == Some(cell.spec.key.as_str()));
        let mut line = card_row(theme, edge)
            .when(highlighted, |d| d.bg(theme.highlight))
            .child(texts);
        for cell in cells {
            let revert = self.revert_button(cell, item, theme, cx);
            let control = div()
                .flex_none()
                .when(!cell.available, |d| d.opacity(0.5))
                .child(self.control(cell, item, true, theme, window, cx));
            line = line.child(
                div()
                    .flex_none()
                    .w(px(column_width(cell.spec.kind)))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(revert_slot(controls::HEIGHT, revert))
                    .child(control),
            );
        }
        items.push(line.into_any_element());
    }

    /// The editable settings shown on a page, grouped by section: while
    /// it is folded, its essentials and what it reveals (ADR-072), then the
    /// fold. Returns how many settings the page has.
    fn page_rows(
        &mut self,
        page: &Page,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let folds = self
            .rows
            .iter()
            .any(|row| row.spec.essential && row.page() == *page);
        let folded = self.folded(page);
        let mut count = 0;
        let rows: Vec<Snapshot> = self
            .rows
            .iter()
            .filter(|row| row.page() == *page)
            .inspect(|_| count += 1)
            .filter(|row| !folded || row.spec.essential || self.revealed.contains(&row.spec.key))
            .map(|row| Snapshot::of(row, self))
            .collect();
        self.section_rows(&rows, items, theme, window, cx);
        let hidden = count - rows.len();
        if folds && (!folded || hidden > 0) {
            self.fold_row(page, folded, hidden, items, theme, cx);
        }
        count
    }

    /// The fold at a page's end: More settings while it shows only its
    /// essentials, Fewer settings once unfolded.
    fn fold_row(
        &mut self,
        page: &Page,
        folded: bool,
        hidden: usize,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) {
        let item = items.len();
        let word = page.word();
        let focus = self.handle(&format!("fold:{word}"), cx);
        self.stop(&focus, Some(item), None);
        let (label, icon) = if folded {
            (format!("More settings ({hidden})"), "chevron-right")
        } else {
            ("Fewer settings".to_owned(), "chevron")
        };
        let page = page.clone();
        items.push(
            div()
                .flex_none()
                .pt_4()
                .flex()
                .child(
                    controls::button(
                        format!("settings-fold-{word}"),
                        label,
                        Some(icon),
                        ButtonStyle::Plain,
                        &focus,
                        theme,
                    )
                    .aria_expanded(!folded)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.fold(&page, !folded, cx);
                    })),
                )
                .into_any_element(),
        );
    }

    /// Settings grouped under their sections' captions, as a page shows
    /// them, sections alike as a table (`settings::blocks`).
    fn section_rows(
        &mut self,
        rows: &[Snapshot],
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let specs: Vec<&SettingSpec> = rows.iter().map(|row| &row.spec).collect();
        for (n, block) in crate::settings::blocks(&specs).into_iter().enumerate() {
            match block {
                Block::Section { title, rows: at } => {
                    if title.is_empty() {
                        items.push(
                            div()
                                .flex_none()
                                .h(px(if n == 0 { 0. } else { 16. }))
                                .into_any_element(),
                        );
                    } else {
                        Self::caption(items, title, theme);
                    }
                    for (index, row) in at.iter().enumerate() {
                        let edge = Edge {
                            first: index == 0,
                            last: index + 1 == at.len(),
                        };
                        self.setting_row(&rows[*row], edge, items, theme, window, cx);
                    }
                }
                Block::Table { columns, lines } => {
                    Self::table_header(&columns, items, theme);
                    for (index, (title, at)) in lines.iter().enumerate() {
                        let edge = Edge {
                            first: index == 0,
                            last: index + 1 == lines.len(),
                        };
                        let cells: Vec<Snapshot> = at
                            .iter()
                            .map(|row| {
                                let mut cell = rows[*row].clone();
                                cell.label = format!("{title}: {}", cell.spec.title).into();
                                cell
                            })
                            .collect();
                        self.table_line(title, &cells, edge, items, theme, window, cx);
                    }
                }
            }
        }
    }

    /// Settings from several pages, each group under its page's title and
    /// section, in the pages' order: what a search finds, what was
    /// changed.
    fn grouped_rows(
        &mut self,
        rows: Vec<Snapshot>,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let mut groups: Vec<((Page, String), Vec<Snapshot>)> = Vec::new();
        for row in rows {
            let at = (Page::of_spec(&row.spec), row.spec.section.clone());
            match groups.last_mut() {
                Some((last, rows)) if *last == at => rows.push(row),
                _ => groups.push((at, vec![row])),
            }
        }
        for ((page, section), rows) in groups {
            let mut caption = self.page_title(&page);
            if !section.is_empty() {
                caption = format!("{caption} › {section}");
            }
            Self::caption(items, caption, theme);
            for (index, row) in rows.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == rows.len(),
                };
                self.setting_row(row, edge, items, theme, window, cx);
            }
        }
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
        // What it may do (ADR-044), and where one from outside skwir is.
        let may = plugin
            .capabilities
            .words()
            .map(|words| format!("May: {words}"));
        let from = plugin
            .origin
            .as_ref()
            .map(|origin| format!("From {origin}"));
        for (icon, text) in [("lock", may), ("plugin", from)] {
            if let Some(text) = text {
                texts = texts.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(controls::glyph(icon, theme.text_muted, 13.))
                        .child(text),
                );
            }
        }
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
        let mut row = card_row(theme, edge).child(texts);
        // To where its settings are, by their subject.
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
                    window.focus(&this.nav, cx);
                    this.go_to_plugin(id, cx);
                })),
            );
        }
        let focus = self.handle(&format!("plugin:{id}"), cx);
        self.stop(&focus, Some(item), None);
        row = row.child(
            controls::switch(format!("plugin-{id}"), shown, &focus, theme)
                .when(self.a11y, |d| d.aria_label(plugin.title.clone()))
                .when(pending, |d| d.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.toggling.contains_key(id) {
                        this.set_plugin(id, !shown, cx);
                    }
                })),
        );
        items.push(row.into_any_element());
    }

    // ------------------------------------------------------------ pages

    fn page_general(
        &mut self,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.page_rows(&Page::General, items, theme, window, cx);
        Self::caption(items, "Settings file", theme);
        let item = items.len();
        let location = self.app.settings.path().map_or_else(
            || "Settings are kept in memory for this session.".to_owned(),
            |path| {
                let shown = skwir_core::os_path::home()
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
            card_row(
                theme,
                Edge {
                    first: true,
                    last: true,
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
                .aria_description("Edit as text")
                .on_click(cx.listener(|this, _, _, _| this.open_file())),
            )
            .into_any_element(),
        );
    }

    /// Changed by you: what a page reached revealed (every setting that
    /// differed from its default, every plugin switched from how skwir
    /// comes), then Restore all.
    fn page_changed(
        &mut self,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let rows: Vec<Snapshot> = self
            .rows
            .iter()
            .filter(|row| self.revealed.contains(&row.spec.key))
            .map(|row| Snapshot::of(row, self))
            .collect();
        let plugins: Vec<PluginInfo> = self
            .plugins
            .iter()
            .filter(|plugin| self.revealed_plugins.contains(plugin.id))
            .cloned()
            .collect();
        if rows.is_empty() && plugins.is_empty() {
            Self::note(
                items,
                "Every setting has its default value, and every plugin is as skwir comes.",
                theme,
            );
            return;
        }
        self.grouped_rows(rows, items, theme, window, cx);
        if !plugins.is_empty() {
            Self::caption(items, "Plugins", theme);
            for (index, plugin) in plugins.iter().enumerate() {
                let edge = Edge {
                    first: index == 0,
                    last: index + 1 == plugins.len(),
                };
                self.plugin_row(plugin, true, edge, items, theme, cx);
            }
        }
        items.push(div().flex_none().h(px(24.)).into_any_element());
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
        let mut row = card_row(
            theme,
            Edge {
                first: true,
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
        self.page_rows(&Page::Plugin(id.to_owned()), items, theme, window, cx);
    }

    fn search_results(
        &mut self,
        query: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // Every setting, folded or not, under its page and section.
        let rows: Vec<Snapshot> = self
            .rows
            .iter()
            .filter(|row| {
                let titles = [
                    self.page_title(&row.page()),
                    self.plugin_title(row.spec.owner()),
                ];
                spec_matches(query, &row.spec, [&titles[0], &titles[1]])
            })
            .map(|row| Snapshot::of(row, self))
            .collect();
        let mut found = rows.len();
        self.grouped_rows(rows, items, theme, window, cx);
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
        found += self.keys_found(query, items, theme, window, cx);
        if found == 0 {
            Self::note(items, format!("Nothing matches “{query}”."), theme);
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.a11y = window.is_a11y_active();
        let theme = cx.global::<Theme>().clone();
        self.stops.clear();
        self.row_items.clear();
        self.record_stops.clear();
        let search_focus = self.search.focus_handle(cx);
        self.stop(&search_focus, None, None);
        let nav = self.nav.clone();
        self.stop(&nav, None, None);
        let sidebar = self.render_sidebar(&theme, window, cx);

        let query = self.query(cx);
        let mut items: Vec<AnyElement> = Vec::new();
        // What the Keyboard page shows above its list, and the list, or the
        // form of a new shortcut, in place of the page's scrolling items.
        let mut toolbar: Option<AnyElement> = None;
        let mut content: Option<AnyElement> = None;
        if self.read_only {
            items.push(
                div()
                    .flex_none()
                    .mt_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.warning)
                    .child(controls::glyph("lock", theme.warning, 14.))
                    .child(div().flex_1().min_w_0().child(
                        "Your settings file is read-only (Home Manager links it, say): \
                         a change here is not kept. Change them where they are declared.",
                    ))
                    .into_any_element(),
            );
        }
        let (title, subtitle): (String, String) = if query.is_empty() {
            let page = self.page.clone();
            let subtitle = page.builtin().map_or("", |b| b.intro);
            match &page {
                Page::General => self.page_general(&mut items, &theme, window, cx),
                Page::Changed => self.page_changed(&mut items, &theme, window, cx),
                Page::Plugins => {
                    for category in Category::ALL {
                        let mut plugins: Vec<PluginInfo> = self
                            .plugins
                            .iter()
                            .filter(|plugin| plugin.category == category)
                            .cloned()
                            .collect();
                        if plugins.is_empty() {
                            continue;
                        }
                        plugins.sort_by_key(|plugin| plugin.title.to_lowercase());
                        Self::caption(&mut items, category.title(), &theme);
                        for (index, plugin) in plugins.iter().enumerate() {
                            let edge = Edge {
                                first: index == 0,
                                last: index + 1 == plugins.len(),
                            };
                            self.plugin_row(plugin, true, edge, &mut items, &theme, cx);
                        }
                    }
                }
                Page::Keyboard => {
                    toolbar = Some(self.keys_toolbar(&theme, cx));
                    if self.keys.new.is_some() {
                        content = Some(self.new_shortcut_page(&theme, window, cx));
                    } else if self.keys.as_records {
                        self.page_rows(&Page::Keyboard, &mut items, &theme, window, cx);
                    } else {
                        content = Some(self.keys_list(window, cx));
                    }
                }
                Page::Plugin(id) => self.page_plugin(id, &mut items, &theme, window, cx),
                page => {
                    if self.page_rows(page, &mut items, &theme, window, cx) == 0 {
                        Self::note(&mut items, "Nothing to adjust here now.", &theme);
                    }
                }
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

        let bar = self.key_editor_bar(&theme, window, cx);
        let close = self.handle("close", cx);
        self.stop(&close, None, None);
        // The save's status follows the title, in the line it already
        // takes: a save never narrows the subtitle.
        let status = if self.saving() {
            Some((None, "Saving…"))
        } else if self.saved_flash {
            Some((Some("check"), "Saved"))
        } else {
            None
        }
        .map(|(icon, text)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .text_xs()
                .text_color(theme.text_muted)
                .children(icon.map(|name| controls::glyph(name, theme.text_muted, 13.)))
                .child(text)
        });
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().text_lg().child(title))
                            .children(status),
                    )
                    .when(!subtitle.is_empty(), |d| {
                        d.child(div().text_xs().text_color(theme.text_muted).child(subtitle))
                    }),
            )
            .child(
                controls::icon_button("settings-close", "close", "Close", &close, &theme)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
            );
        // The read-only warning stays above a page that is not its items.
        let warning = (content.is_some() && !items.is_empty()).then(|| {
            div()
                .flex_none()
                .px_6()
                .pb_2()
                .children(items.drain(..))
                .into_any_element()
        });
        let content = content.unwrap_or_else(|| {
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
            // A column, like the one the body used to sit in directly: the
            // page keeps the height it scrolls within.
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
                ))
                .into_any_element()
        });
        let panel = div()
            .id("settings")
            .role(gpui::Role::Dialog)
            .aria_modal()
            .aria_label("Settings")
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
                    .children(warning)
                    .children(toolbar)
                    .child(content)
                    .children(bar),
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
