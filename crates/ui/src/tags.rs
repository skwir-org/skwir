//! Tags in the window: the colors they wear, their chips, and the tag
//! editor (`ui.tags_prompt`, Ctrl+Shift+G).
//!
//! A tag wears the color the user chose for it (`tags.colors`, the tags
//! plugin's, read when the settings change), else the one its file gives
//! it (a Mac's Finder tags carry one: `keys::TAG_COLORS`), else the one
//! its name gives it (`skwir_core::metadata::tag_color`), so every view
//! agrees without storing anything of ours per file.
//!
//! The editor acts on the entries selected when it opened, whatever the
//! selection is by the time it applies. It reads their tags from the files
//! on a worker (`tags.selected`) and lists each tag with whether it is on
//! all of them, on some, or on none, then the tags the user has used,
//! most recent first. A click or Enter on a row turns a tag on for all,
//! off for all, and back to as it was on some; typed text filters the
//! rows and Enter on it gives the new tag. What applies is the difference
//! (`tags.edit`): each entry keeps the tags nobody touched, so a selection
//! of files tagged differently never gains the others' tags.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, Subscription, Window, div, prelude::*, px,
};
use skwir_core::command::Outcome;
use skwir_core::metadata::{
    TAG_COLOR_SETTING, TAG_COLORS, same_tag, tag_color, tag_leaf, tag_lineage, tag_parent,
};
use skwir_core::op::EntryRef;
use skwir_core::workspace::PaneId;

use crate::controls::{self, ButtonStyle};
use crate::elements::clip::clipped;
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn, Picker};
use crate::shell::Shell;
use crate::theme::Theme;

/// The colors the user chose, by tag in lowercase.
#[derive(Clone, Debug, Default)]
pub struct TagColors(HashMap<String, usize>);

impl TagColors {
    pub fn read(app: &skwir_core::App) -> Self {
        let table: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
        TagColors(
            table
                .into_iter()
                .filter_map(|(tag, color)| {
                    let index = TAG_COLORS.iter().position(|c| Some(*c) == color.as_str())?;
                    Some((tag, index))
                })
                .collect(),
        )
    }

    /// The color `tag` wears, as an index into `Theme::tags`.
    pub fn of(&self, tag: &str) -> usize {
        self.of_or(tag, None)
    }

    /// The color `tag` wears where its file gives it `given`: the user's
    /// choice, else the file's, else the one the user chose for its
    /// nearest category, else its name's.
    pub fn of_or(&self, tag: &str, given: Option<usize>) -> usize {
        let chosen = |t: &str| self.0.get(&t.to_lowercase()).copied();
        chosen(tag)
            .or(given)
            .or_else(|| tag_lineage(tag).skip(1).find_map(chosen))
            .unwrap_or_else(|| tag_color(tag))
    }

    /// The color `tag` wears on a file that gives its tags `given`.
    pub fn of_given(&self, tag: &str, given: &[(String, usize)]) -> usize {
        let given = given
            .iter()
            .find(|(t, _)| same_tag(t, tag))
            .map(|(_, c)| *c);
        self.of_or(tag, given)
    }

    /// Whether the user chose `tag`'s color.
    fn chosen(&self, tag: &str) -> bool {
        self.0.contains_key(&tag.to_lowercase())
    }
}

/// The colors an entry's file gives its tags (`keys::TAG_COLORS`), as
/// indices into `Theme::tags`.
pub fn given_colors(e: &skwir_core::Entry) -> Vec<(String, usize)> {
    use skwir_core::Value;
    let Some(Value::List(pairs)) = e.metadata.value(skwir_core::entry::keys::TAG_COLORS) else {
        return Vec::new();
    };
    pairs
        .iter()
        .filter_map(|pair| {
            let Value::List(pair) = pair else { return None };
            let tag = pair.first()?.as_str()?;
            let color = pair.get(1)?.as_str()?;
            let index = TAG_COLORS.iter().position(|c| *c == color)?;
            Some((tag.to_owned(), index))
        })
        .collect()
}

/// Tags from the entry's `tags` metadata field (a list of text values).
pub fn tags_of(e: &skwir_core::Entry) -> Vec<String> {
    match e.metadata.value(skwir_core::entry::keys::TAGS) {
        Some(skwir_core::Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(skwir_core::Value::Text(t)) => vec![t.to_string()],
        _ => Vec::new(),
    }
}

/// An entry's tags, each with the color it wears.
pub fn entry_tags(e: &skwir_core::Entry, colors: &TagColors) -> Vec<(String, usize)> {
    let given = given_colors(e);
    tags_of(e)
        .into_iter()
        .map(|t| {
            let color = colors.of_given(&t, &given);
            (t, color)
        })
        .collect()
}

/// A chip's height: rows of chips are one chip tall (the list's wrap).
pub const CHIP_HEIGHT: f32 = 18.;

/// A tag as a chip, in its color: its own name (`Paris` for
/// `Places/France/Paris`), as a row has room for.
pub fn chip(tag: &str, color: usize, theme: &Theme) -> Div {
    chip_frame(color, theme).child(tag_leaf(tag).to_owned())
}

/// A tag as a chip with its categories before its name, dimmed, where
/// there is room for them (Properties).
pub fn chip_in_full(tag: &str, color: usize, theme: &Theme) -> Div {
    match tag_parent(tag) {
        Some(categories) => chip_frame(color, theme)
            .child(div().opacity(0.6).child(format!("{categories}/")))
            .child(tag_leaf(tag).to_owned()),
        None => chip(tag, color, theme),
    }
}

fn chip_frame(color: usize, theme: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(CHIP_HEIGHT))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .px(px(5.))
        .rounded_sm()
        .text_xs()
        .bg(theme.tag_fill(color))
}

/// How many tags a line left out, as a chip without a color.
pub fn more_chip(n: usize, color: gpui::Hsla) -> Div {
    div()
        .flex_none()
        .h(px(CHIP_HEIGHT))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .px(px(3.))
        .text_xs()
        .text_color(color)
        .child(format!("+{n}"))
}

/// The most dots a grid tile shows before "+N".
const TILE_DOTS: usize = 3;

/// A tile's tags as dots over the top corner of its picture, as many as
/// `TILE_DOTS` and then how many more, their names in the tooltip. Over
/// the picture, not under the name: the tile keeps the height its name
/// gives it (`GridView::measure`), tagged or not. `None` for no tags.
pub fn tile_dots(
    id: impl Into<gpui::ElementId>,
    tags: &[(String, usize)],
    zoom: f32,
    theme: &Theme,
) -> Option<AnyElement> {
    if tags.is_empty() {
        return None;
    }
    // Growing slower than the picture, like the link's badge.
    let side = (zoom * 0.04 + 7.).round().clamp(10., 16.);
    let ring = theme.bg;
    let more = tags.len().saturating_sub(TILE_DOTS);
    let tip: gpui::SharedString = tags
        .iter()
        .map(|(tag, _)| tag.as_str())
        .collect::<Vec<_>>()
        .join(", ")
        .into();
    Some(
        div()
            .id(id)
            .absolute()
            .top(px(-3.))
            .left(px(-3.))
            .flex()
            .items_center()
            .children(
                tags.iter()
                    .take(TILE_DOTS)
                    .enumerate()
                    .map(|(i, (_, color))| {
                        // Each overlaps the one before it by a third, ringed so
                        // they part on any picture.
                        div()
                            .flex_none()
                            .size(px(side))
                            .when(i > 0, |d| d.ml(px(-side / 3.)))
                            .rounded_full()
                            .border_2()
                            .border_color(ring)
                            .bg(theme.tag(*color))
                    }),
            )
            .when(more > 0, |d| {
                d.child(
                    div()
                        .ml(px(2.))
                        .px(px(3.))
                        .rounded_sm()
                        .bg(ring)
                        .text_color(theme.text_muted)
                        .text_size(px(10.))
                        .line_height(px(side))
                        .child(format!("+{more}")),
                )
            })
            .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
            .into_any_element(),
    )
}

/// A tag's dot, in its color.
pub(crate) fn dot(color: usize, theme: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(10.))
        .rounded_full()
        .bg(theme.tag(color))
}

/// Star `n` of five for a rating in half stars: filled, half, or empty.
pub fn star(n: u64, rating: u64, size: f32, theme: &Theme) -> gpui::Svg {
    let (icon, color) = if rating >= n * 2 {
        ("star-filled", theme.rating)
    } else if rating + 1 == n * 2 {
        ("star-half", theme.rating)
    } else {
        ("star", theme.border)
    };
    controls::glyph(icon, color, size)
}

/// A rating as five stars.
pub fn stars(rating: u64, size: f32, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(1.))
        .children((1..=5).map(|n| star(n, rating, size, theme)))
}

/// A rating in words: "4 stars", "3½ stars".
pub fn stars_words(rating: u64) -> String {
    let whole = rating / 2;
    let half = if rating % 2 == 1 { "½" } else { "" };
    let plural = if rating <= 2 { "" } else { "s" };
    format!("{whole}{half} star{plural}").replacen("0½", "½", 1)
}

/// Where a tag is, or will be, among the entries edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    All,
    Some,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagRow {
    pub name: String,
    /// On how many entries it was when the editor opened.
    pub count: usize,
    pub mark: Mark,
    /// The color a file gives it (a Finder tag's).
    pub given: Option<usize>,
}

impl TagRow {
    fn was(&self, items: usize) -> Mark {
        match self.count {
            0 => Mark::None,
            n if n >= items => Mark::All,
            _ => Mark::Some,
        }
    }

    /// A click: on for all, off for all, then back to some if it was.
    fn toggle(&mut self, items: usize) {
        self.mark = match (self.mark, self.was(items)) {
            (Mark::All, _) => Mark::None,
            (Mark::None, Mark::Some) => Mark::Some,
            (Mark::None | Mark::Some, _) => Mark::All,
        };
    }
}

/// A row of the list shown: a tag, or giving the tags typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    Row(usize),
    New(Vec<String>),
}

/// The rows matching `query` (every row when it is empty), a tag spelled
/// as typed first; before them, giving what was typed when some of it is
/// no row yet.
pub fn shown(rows: &[TagRow], query: &str) -> Vec<Shown> {
    // `Places/` asks what is in the category: nothing to add yet.
    let typed = if query
        .trim_end()
        .ends_with(skwir_core::metadata::TAG_SEPARATOR)
    {
        Vec::new()
    } else {
        split(query)
    };
    let needle = query.trim().to_lowercase();
    let mut out = Vec::new();
    let new: Vec<String> = typed
        .iter()
        .filter(|t| !rows.iter().any(|r| same_tag(&r.name, t)))
        .cloned()
        .collect();
    if !new.is_empty() {
        out.push(Shown::New(new));
    }
    let mut matching: Vec<usize> = (0..rows.len())
        .filter(|&i| needle.is_empty() || rows[i].name.to_lowercase().contains(&needle))
        .collect();
    // The tag typed exactly comes first, for Enter.
    if let Some(at) = matching
        .iter()
        .position(|&i| same_tag(&rows[i].name, query.trim()))
    {
        let exact = matching.remove(at);
        matching.insert(0, exact);
    }
    out.extend(matching.into_iter().map(Shown::Row));
    out
}

/// Tags typed, separated by commas, each made one (`Places / France` is
/// `Places/France`).
fn split(text: &str) -> Vec<String> {
    skwir_core::notes::parse_tags(text)
}

/// A tag of a field (a category holding one tag at a time, `fields`)
/// given: the field's other values are taken, as the edit will.
fn choose_one(rows: &mut [TagRow], picked: usize, fields: &[String]) {
    use skwir_core::metadata::exclusive_category;
    let Some(row) = rows.get(picked).filter(|r| r.mark == Mark::All) else {
        return;
    };
    let Some(field) = exclusive_category(&row.name, fields).map(str::to_owned) else {
        return;
    };
    for (i, other) in rows.iter_mut().enumerate() {
        if i != picked
            && exclusive_category(&other.name, fields).is_some_and(|f| same_tag(f, &field))
        {
            other.mark = Mark::None;
        }
    }
}

/// What to give every entry and what to take from every entry: the rows
/// whose mark moved.
pub fn changes(rows: &[TagRow], items: usize) -> (Vec<String>, Vec<String>) {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for row in rows.iter().filter(|r| r.mark != r.was(items)) {
        match row.mark {
            Mark::All => add.push(row.name.clone()),
            Mark::None => remove.push(row.name.clone()),
            Mark::Some => {}
        }
    }
    (add, remove)
}

/// What `tags.selected` answers.
#[derive(Debug, Default, serde::Deserialize)]
struct Selected {
    items: usize,
    tags: Vec<Counted>,
    known: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Counted {
    name: String,
    count: usize,
    #[serde(default)]
    color: Option<String>,
}

/// The rows the editor opens with: the entries' tags, then the ones the
/// user has used.
fn rows_of(selected: Selected) -> Vec<TagRow> {
    let items = selected.items;
    let mut rows: Vec<TagRow> = selected
        .tags
        .into_iter()
        .map(|t| TagRow {
            mark: if t.count >= items {
                Mark::All
            } else {
                Mark::Some
            },
            given: t
                .color
                .as_deref()
                .and_then(|c| TAG_COLORS.iter().position(|x| *x == c)),
            name: t.name,
            count: t.count,
        })
        .collect();
    for name in selected.known {
        if !rows.iter().any(|r| same_tag(&r.name, &name)) {
            rows.push(TagRow {
                name,
                count: 0,
                mark: Mark::None,
                given: None,
            });
        }
    }
    rows
}

pub struct TagEditor {
    pub input: Entity<TextInput>,
    /// The entries edited, as they were selected when it opened.
    entries: Vec<EntryRef>,
    pane: PaneId,
    title: String,
    items: usize,
    rows: Vec<TagRow>,
    /// The tags arrived from the files.
    loaded: bool,
    shown: Vec<Shown>,
    /// The query `shown` was computed for.
    query: Option<String>,
    picker: Picker,
    /// The row whose colors are open.
    colors_of: Option<usize>,
    request: u64,
    apply_focus: FocusHandle,
    cancel_focus: FocusHandle,
    back: FocusReturn,
    _sub: Subscription,
}

impl TagEditor {
    /// What Tab walks: the field, Cancel, Apply (`Shell::walk_dialog`).
    pub(crate) fn walk(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        use gpui::Focusable as _;
        vec![
            self.input.read(cx).focus_handle(cx),
            self.cancel_focus.clone(),
            self.apply_focus.clone(),
        ]
    }
}

impl Shell {
    /// `ui.tags_prompt { entries?, text? }`: edit the tags of `entries`
    /// (Properties names the entry it shows), or of what is selected, `text`
    /// typed already (`Status/`: a field's values).
    pub fn open_tag_editor(
        &mut self,
        entries: Option<Vec<EntryRef>>,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        static REQUESTS: AtomicU64 = AtomicU64::new(0);
        if self.app.commands.get("tags.selected").is_none() {
            self.set_status(
                "tags are off: turn the Tags plugin on in Settings",
                true,
                cx,
            );
            return;
        }
        let pane = self.pane;
        let entries = entries.unwrap_or_else(|| self.app.selected_refs(pane));
        if entries.is_empty() {
            return;
        }
        let title = match entries.as_slice() {
            [one] => format!(
                "Tags of “{}”",
                one.path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ),
            many => format!("Tags of {}", crate::format::count(many.len(), "item")),
        };
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new("Filter, or type a new tag", cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.tag_editor_enter(window, cx),
                InputEvent::Cancel => this.close_tag_editor(window, cx),
                InputEvent::Changed => {
                    if let Some(e) = &mut this.tag_editor {
                        e.colors_of = None;
                    }
                    cx.notify();
                }
            },
        );
        input.update(cx, |i, cx| {
            if let Some(text) = text {
                i.set_text(text, cx);
            }
            i.focus(window, cx);
        });
        let request = REQUESTS.fetch_add(1, Ordering::Relaxed);
        let items = entries.len();
        self.tag_editor = Some(TagEditor {
            input,
            entries: entries.clone(),
            pane,
            title,
            items,
            rows: Vec::new(),
            loaded: false,
            shown: Vec::new(),
            query: None,
            picker: Picker::default(),
            colors_of: None,
            request,
            apply_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            back,
            _sub: sub,
        });
        let app = std::sync::Arc::clone(&self.app);
        self.off_thread(
            "tags-selected",
            move || {
                app.dispatch(&skwir_core::command::Invocation::with_args(
                    "tags.selected",
                    &serde_json::json!({ "entries": entries }),
                ))
            },
            move |this, result, window, cx| {
                if this
                    .tag_editor
                    .as_ref()
                    .is_none_or(|e| e.request != request)
                {
                    return;
                }
                let selected = match result {
                    Ok(Outcome::Value(value)) => serde_json::from_value::<Selected>(value)
                        .map_err(|e| anyhow::anyhow!("unreadable tags: {e}")),
                    Ok(_) => Err(anyhow::anyhow!("no tags")),
                    Err(e) => Err(e),
                };
                match selected {
                    Ok(selected) => {
                        if let Some(e) = &mut this.tag_editor {
                            e.items = selected.items.max(1);
                            e.rows = rows_of(selected);
                            e.loaded = true;
                            e.query = None;
                        }
                    }
                    Err(error) => {
                        this.close_tag_editor(window, cx);
                        this.report_error(&error, cx);
                    }
                }
                cx.notify();
            },
            window,
            cx,
        );
        cx.notify();
    }

    pub fn close_tag_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.tag_editor.take() {
            editor.back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
    }

    /// `ui.tag_editor_move`.
    pub fn tag_editor_move(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        self.tag_editor_refilter(cx);
        if let Some(e) = &mut self.tag_editor {
            e.picker.go(mv, e.shown.len());
            cx.notify();
        }
    }

    /// Enter in the field: with text, give or toggle what is highlighted;
    /// without, apply.
    fn tag_editor_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tag_editor_refilter(cx);
        let typed = self
            .tag_editor
            .as_ref()
            .is_some_and(|e| !e.input.read(cx).text().trim().is_empty());
        if typed {
            let highlighted = self
                .tag_editor
                .as_ref()
                .and_then(|e| e.shown.get(e.picker.highlighted).cloned());
            if let Some(shown) = highlighted {
                self.tag_editor_pick(&shown, cx);
            }
            if let Some(e) = &self.tag_editor {
                e.input.update(cx, |i, cx| i.set_text("", cx));
            }
        } else {
            self.tag_editor_apply(window, cx);
        }
    }

    /// A row chosen: toggle a tag, or give the new ones on every entry.
    fn tag_editor_pick(&mut self, shown: &Shown, cx: &mut Context<Self>) {
        let Some(e) = &mut self.tag_editor else {
            return;
        };
        if !e.loaded {
            return;
        }
        let fields = self.tag_fields.clone();
        match shown {
            Shown::Row(i) => {
                let items = e.items;
                if let Some(row) = e.rows.get_mut(*i) {
                    row.toggle(items);
                }
                choose_one(&mut e.rows, *i, &fields);
            }
            Shown::New(tags) => {
                // After the entries' own tags, before those only used.
                let at = e
                    .rows
                    .iter()
                    .position(|r| r.count == 0 && r.mark == Mark::None);
                let at = at.unwrap_or(e.rows.len());
                for (n, tag) in tags.iter().enumerate() {
                    e.rows.insert(
                        at + n,
                        TagRow {
                            name: tag.clone(),
                            count: 0,
                            mark: Mark::All,
                            given: None,
                        },
                    );
                    choose_one(&mut e.rows, at + n, &fields);
                }
            }
        }
        e.query = None;
        cx.notify();
    }

    /// `ui.tag_editor_apply`: give what was turned on, take what was
    /// turned off, as one plan; the text still in the field is given too.
    pub fn tag_editor_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tag_editor_refilter(cx);
        let pending = self.tag_editor.as_ref().and_then(|e| {
            let text = e.input.read(cx).text().to_owned();
            (!text.trim().is_empty()).then(|| e.shown.first().cloned())?
        });
        if let Some(Shown::New(tags)) = pending {
            self.tag_editor_pick(&Shown::New(tags), cx);
        }
        let Some(editor) = self.tag_editor.as_ref().filter(|e| e.loaded) else {
            return;
        };
        let (add, remove) = changes(&editor.rows, editor.items);
        let (entries, pane) = (editor.entries.clone(), editor.pane);
        self.close_tag_editor(window, cx);
        if add.is_empty() && remove.is_empty() {
            return;
        }
        self.run(
            "tags.edit",
            serde_json::json!({ "add": add, "remove": remove, "entries": entries, "pane": pane }),
            window,
            cx,
        );
    }

    fn tag_editor_refilter(&mut self, cx: &mut Context<Self>) {
        let Some(e) = &mut self.tag_editor else {
            return;
        };
        let q = e.input.read(cx).text().to_owned();
        if e.query.as_deref() == Some(q.as_str()) {
            return;
        }
        e.shown = shown(&e.rows, &q);
        e.picker = Picker::default();
        e.query = Some(q);
    }
}

/// The check box of a mark: ticked, a dash for some, empty.
fn mark_box(mark: Mark, theme: &Theme) -> Div {
    let filled = mark != Mark::None;
    div()
        .flex_none()
        .size(px(16.))
        .rounded_sm()
        .border_1()
        .border_color(if filled {
            theme.accent
        } else {
            theme.text_muted
        })
        .when(filled, |d| d.bg(theme.accent))
        .flex()
        .items_center()
        .justify_center()
        .child(match mark {
            Mark::All => controls::glyph("check", theme.on_accent, 12.).into_any_element(),
            Mark::Some => div()
                .w(px(8.))
                .h(px(2.))
                .bg(theme.on_accent)
                .into_any_element(),
            Mark::None => div().into_any_element(),
        })
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.tag_editor_refilter(cx);
    let theme = cx.global::<Theme>().clone();
    let colors = shell.tag_colors.clone();
    let apply_keys = shell.keys_for("ui.tag_editor_apply", &serde_json::json!({}));
    let e = shell.tag_editor.as_ref()?;
    let a11y = shell.a11y;
    let typing = gpui::Focusable::focus_handle(e.input.read(cx), cx);
    let mut list = modal::options("tag-rows", "Tags", e.shown.len(), a11y)
        .flex()
        .flex_col()
        .py_1()
        .overflow_hidden();
    for row in e.picker.shown(e.shown.len()) {
        let hl = row == e.picker.highlighted;
        let shown = e.shown[row].clone();
        // A tag says whether the files have it: all, some, none.
        let (named, marked) = match &shown {
            Shown::New(tags) => (
                a11y.then(|| format!("Add {}", tags.join(", ")).into()),
                None,
            ),
            Shown::Row(i) => (
                a11y.then(|| e.rows[*i].name.clone().into()),
                Some(match e.rows[*i].mark {
                    Mark::All => gpui::Toggled::True,
                    Mark::Some => gpui::Toggled::Mixed,
                    Mark::None => gpui::Toggled::False,
                }),
            ),
        };
        // Known by the tag it shows, or the tags it adds.
        let known = match &shown {
            Shown::New(tags) => crate::elements::known_by("tag-new", tags),
            Shown::Row(i) => crate::elements::known_by("tag-row", &e.rows[*i].name),
        };
        let line = modal::option(div().id(known), named, row, hl, &typing)
            .when_some(
                marked.filter(|_| a11y),
                StatefulInteractiveElement::aria_toggled,
            )
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(hl, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!hl, |d| d.hover(|s| s.bg(theme.hover)))
            .on_click(cx.listener({
                let shown = shown.clone();
                move |this, _, window, cx| {
                    this.tag_editor_pick(&shown, cx);
                    if let Some(e) = &this.tag_editor {
                        e.input.update(cx, |i, cx| i.focus(window, cx));
                    }
                }
            }));
        let muted = if hl {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let line = match &shown {
            Shown::New(tags) => line
                .child(controls::glyph("plus", muted, 14.))
                .child(clipped(
                    div().id("tag-new").flex_1().min_w_0().truncate(),
                    format!(
                        "Add {}",
                        tags.iter()
                            .map(|t| format!("“{t}”"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )),
            Shown::Row(i) => {
                let tag = &e.rows[*i];
                let color = colors.of_or(&tag.name, tag.given);
                let index = *i;
                let detail = match (tag.mark, tag.was(e.items)) {
                    (Mark::Some, _) => format!("on {} of {}", tag.count, e.items),
                    (Mark::All, Mark::All) | (Mark::None, Mark::None) => String::new(),
                    (Mark::All, _) => "added".into(),
                    (Mark::None, _) => "removed".into(),
                };
                line.child(mark_box(tag.mark, &theme))
                    .child(
                        // The dot opens the tag's colors; the row stays
                        // as it is.
                        div()
                            .id(("tag-dot", row))
                            .p_1()
                            .rounded_full()
                            .hover(|s| s.bg(theme.hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                if let Some(e) = &mut this.tag_editor {
                                    e.colors_of = (e.colors_of != Some(index)).then_some(index);
                                    e.input.update(cx, |i, cx| i.focus(window, cx));
                                }
                                cx.notify();
                            }))
                            .child(dot(color, &theme)),
                    )
                    // Its own name, then the category it is in.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(clipped(
                                div().id("tag-name").flex_none().max_w_full().truncate(),
                                tag_leaf(&tag.name).to_owned(),
                            ))
                            .when_some(tag_parent(&tag.name), |d, category| {
                                d.child(clipped(
                                    div()
                                        .id("tag-category")
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(muted),
                                    category.to_owned(),
                                ))
                            }),
                    )
                    .child(div().flex_none().text_xs().text_color(muted).child(detail))
            }
        };
        list = list.child(line);
        if let Shown::Row(i) = shown
            && e.colors_of == Some(i)
        {
            let row = &e.rows[i];
            list = list.child(color_strip(&row.name, row.given, &colors, &theme, cx));
        }
    }
    if e.loaded && e.shown.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_color(theme.text_muted)
                .child("No tags yet: type one and press Enter"),
        );
    }
    let footer = modal::footer(
        "Enter on an empty field applies",
        [
            controls::button(
                "tags-cancel",
                "Cancel",
                None,
                ButtonStyle::Plain,
                &e.cancel_focus,
                &theme,
            )
            .on_click(cx.listener(|this, _, window, cx| this.close_tag_editor(window, cx))),
            controls::button(
                "tags-apply",
                "Apply",
                Some("check"),
                ButtonStyle::Plain,
                &e.apply_focus,
                &theme,
            )
            .when_some(apply_keys, |d, keys| {
                d.tooltip(move |_, cx| {
                    crate::toolbar::tooltip(format!("Apply ({keys})").into(), cx)
                })
            })
            .on_click(cx.listener(|this, _, window, cx| this.tag_editor_apply(window, cx))),
        ],
        &theme,
    )
    .px_3()
    .py_2()
    .border_t_1()
    .border_color(theme.border);
    let panel = modal::panel("tag-editor", 440., &theme)
        .aria_label("Tag editor")
        .key_context(keymap::TAG_EDITOR)
        .child(
            div()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
                .child(clipped(
                    div()
                        .id("tags-title")
                        .px_1()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(theme.text_muted),
                    e.title.clone(),
                ))
                .child(e.input.clone()),
        )
        .child(list)
        .child(footer);
    Some(modal::modal(
        panel,
        80.,
        4,
        Shell::close_tag_editor,
        &theme,
        cx,
    ))
}

/// The colors `tag` can wear, under its row: a click chooses one for it
/// everywhere (`tags.color`), Automatic gives back its name's.
fn color_strip(
    tag: &str,
    given: Option<usize>,
    colors: &TagColors,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Div {
    let current = colors.of_or(tag, given);
    let chosen = colors.chosen(tag);
    let mut strip = div()
        .pl(px(52.))
        .pr_3()
        .pb_1()
        .flex()
        .items_center()
        .gap_1();
    for (index, name) in TAG_COLORS.iter().enumerate() {
        let tag = tag.to_owned();
        strip = strip.child(
            div()
                .id(("tag-color", index))
                .p(px(3.))
                .rounded_full()
                .border_1()
                .border_color(if chosen && index == current {
                    theme.text
                } else {
                    gpui::transparent_black()
                })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(
                        "tags.color",
                        serde_json::json!({ "tag": tag, "color": name }),
                        window,
                        cx,
                    );
                    if let Some(e) = &this.tag_editor {
                        e.input.update(cx, |i, cx| i.focus(window, cx));
                    }
                }))
                .child(dot(index, theme).size(px(14.))),
        );
    }
    let tag = tag.to_owned();
    strip.child(
        div()
            .id("tag-color-auto")
            .ml_2()
            .px_2()
            .rounded_sm()
            .text_xs()
            .text_color(if chosen {
                theme.accent
            } else {
                theme.text_muted
            })
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.run("tags.color", serde_json::json!({ "tag": tag }), window, cx);
                if let Some(e) = &this.tag_editor {
                    e.input.update(cx, |i, cx| i.focus(window, cx));
                }
            }))
            .child("Automatic"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, count: usize) -> TagRow {
        TagRow {
            name: name.into(),
            count,
            mark: match count {
                0 => Mark::None,
                3 => Mark::All,
                _ => Mark::Some,
            },
            given: None,
        }
    }

    #[test]
    fn a_click_cycles_through_what_the_tag_can_be() {
        let mut some = row("draft", 1);
        some.toggle(3);
        assert_eq!(some.mark, Mark::All);
        some.toggle(3);
        assert_eq!(some.mark, Mark::None);
        some.toggle(3);
        assert_eq!(some.mark, Mark::Some, "back to as it was");
        let mut all = row("work", 3);
        all.toggle(3);
        assert_eq!(all.mark, Mark::None);
        all.toggle(3);
        assert_eq!(all.mark, Mark::All, "never some: it was on every entry");
    }

    /// Nothing touched changes nothing; a tag on some entries is only
    /// spread when asked.
    #[test]
    fn only_what_moved_is_applied() {
        let mut rows = vec![row("work", 3), row("draft", 1), row("home", 0)];
        assert_eq!(changes(&rows, 3), (vec![], vec![]));
        rows[0].toggle(3);
        rows[2].toggle(3);
        assert_eq!(
            changes(&rows, 3),
            (vec!["home".to_owned()], vec!["work".to_owned()])
        );
        rows[1].toggle(3);
        assert_eq!(changes(&rows, 3).0, ["draft", "home"]);
    }

    #[test]
    fn a_field_takes_one_of_its_tags() {
        let mut rows = vec![
            row("Status/Draft", 1),
            row("Status/Final", 0),
            row("work", 1),
        ];
        rows[0].mark = Mark::All;
        rows[1].mark = Mark::All;
        choose_one(&mut rows, 1, &["status".to_owned()]);
        assert_eq!(
            rows.iter().map(|r| r.mark).collect::<Vec<_>>(),
            [Mark::None, Mark::All, Mark::Some]
        );
        // Not a field: several stay.
        rows[0].mark = Mark::All;
        choose_one(&mut rows, 0, &[]);
        assert_eq!(rows[1].mark, Mark::All);
    }

    #[test]
    fn typing_filters_and_offers_what_is_new() {
        let rows = vec![row("Work", 3), row("homework", 0), row("draft", 1)];
        assert_eq!(
            shown(&rows, ""),
            [Shown::Row(0), Shown::Row(1), Shown::Row(2)]
        );
        assert_eq!(
            shown(&rows, "work"),
            [Shown::Row(0), Shown::Row(1)],
            "the tag typed exactly first, nothing new"
        );
        assert_eq!(
            shown(&rows, "wor"),
            [Shown::New(vec!["wor".into()]), Shown::Row(0), Shown::Row(1)]
        );
        assert_eq!(
            shown(&rows, "draft, urgent, Urgent"),
            [Shown::New(vec!["urgent".into()])]
        );
        // A category typed lists what is in it; a tag in one is made one.
        let rows = vec![row("Places/France", 1), row("work", 0)];
        assert_eq!(shown(&rows, "Places/"), [Shown::Row(0)]);
        assert_eq!(
            shown(&rows, "places / Italy"),
            [Shown::New(vec!["places/Italy".into()])]
        );
        assert_eq!(
            shown(&rows, "fra"),
            [Shown::New(vec!["fra".into()]), Shown::Row(0)]
        );
    }

    #[test]
    fn the_rows_start_with_the_entries_tags_then_the_known() {
        let rows = rows_of(Selected {
            items: 2,
            tags: vec![
                Counted {
                    name: "Work".into(),
                    count: 2,
                    color: None,
                },
                Counted {
                    name: "draft".into(),
                    count: 1,
                    color: Some("blue".into()),
                },
            ],
            known: vec!["home".into(), "work".into()],
        });
        assert_eq!(rows[1].given, TAG_COLORS.iter().position(|c| *c == "blue"));
        let marks: Vec<(&str, Mark)> = rows.iter().map(|r| (r.name.as_str(), r.mark)).collect();
        assert_eq!(
            marks,
            [
                ("Work", Mark::All),
                ("draft", Mark::Some),
                ("home", Mark::None)
            ]
        );
    }

    #[test]
    fn a_tag_wears_the_color_chosen_or_its_names() {
        let mut colors = TagColors::default();
        assert_eq!(colors.of("Work"), tag_color("work"));
        colors.0.insert("work".into(), 4);
        assert_eq!(colors.of("WORK"), 4);
        assert!(colors.chosen("Work") && !colors.chosen("home"));
        // A file's color (a Finder tag's) comes before the name's, never
        // before the user's.
        let given = [("Home".to_owned(), 6), ("work".to_owned(), 1)];
        assert_eq!(colors.of_given("home", &given), 6);
        assert_eq!(colors.of_given("Work", &given), 4);
        assert_eq!(colors.of_given("other", &given), tag_color("other"));
    }
}
